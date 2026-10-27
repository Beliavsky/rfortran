"""Private derived components: module/host association, constructors and imports."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from run_ofort_compat import equivalent
from test_record_read import ROOT, COMPILER

MODULE = '''module boxes
implicit none
type :: box
private
integer :: value = 3
contains
procedure :: get
procedure :: set
end type
contains
integer function get(this)
class(box),intent(in)::this
get=this%value
end function
subroutine set(this,n)
class(box),intent(inout)::this
integer,intent(in)::n
this%value=n
end subroutine
function make(n) result(x)
integer,intent(in)::n
type(box)::x
x%value=n
end function
end module
'''


class PrivateComponents(unittest.TestCase):
    def invoke(self, command, cwd):
        return subprocess.run(list(map(str,command)),cwd=cwd,input='',capture_output=True,text=True,timeout=60)

    def check(self, source, expected):
        with tempfile.TemporaryDirectory(prefix='private-components-') as tmp:
            directory=Path(tmp)
            src=directory/'example.f90';src.write_text(source)
            for compiler in [COMPILER]+(['gfortran'] if shutil.which('gfortran') else []):
                if compiler==COMPILER:
                    r=self.invoke([compiler,'run',src],directory)
                    self.assertEqual(r.returncode,0,r.stderr)
                    self.assertTrue(equivalent(r.stdout,expected),r.stdout)
                for opt in ([0,2] if compiler==COMPILER else [0]):
                    exe=directory/('p.exe' if os.name=='nt' else 'p')
                    r=self.invoke([compiler,f'-O{opt}',src,'-o',exe],directory)
                    self.assertEqual(r.returncode,0,r.stderr)
                    r=self.invoke([exe],directory)
                    self.assertEqual(r.returncode,0,r.stderr)
                    self.assertTrue(equivalent(r.stdout,expected),r.stdout)

    def reject(self, source, message='is PRIVATE to module'):
        with tempfile.TemporaryDirectory(prefix='private-invalid-') as tmp:
            directory=Path(tmp);src=directory/'example.f90';src.write_text(source)
            for command in [[COMPILER,'run',src],[COMPILER,'-c',src]]:
                r=self.invoke(command,directory)
                self.assertNotEqual(r.returncode,0,r.stdout)
                self.assertIn(message,r.stderr)
                self.assertRegex(r.stderr,r'example\.f90:\d+:\d+')
            if shutil.which('gfortran'):
                r=self.invoke(['gfortran','-fsyntax-only',src],directory)
                self.assertNotEqual(r.returncode,0,'reference unexpectedly accepts source')

    def test_fixture(self):
        self.check((ROOT/"tests/programs/xprivate_components.f90").read_text(),"7 7\n3")

    def test_public_bindings_factory_and_whole_assignment(self):
        self.check(MODULE+'''program p
use boxes
implicit none
type(box)::a,b
a=make(7)
b=a
call b%set(9)
print *,a%get(),b%get()
end
''','7 9')

    def test_default_constructor_omits_private_component(self):
        self.check(MODULE+'''program p
use boxes
type(box)::a
a=box()
print *,a%get()
end
''','3')

    def test_component_attribute_without_default_private(self):
        module=MODULE.replace('private\ninteger :: value = 3','integer,private :: value = 3')
        self.check(module+'program p\nuse boxes\ntype(box)::a\na=make(8)\nprint *,a%get()\nend\n','8')
        self.reject(module+'program p\nuse boxes\ntype(box)::a\nprint *,a%value\nend\n')

    def test_private_scalar_read_write_and_actual(self):
        for statement in ['print *,a%value','a%value=8','call random_seed(size=a%value)']:
            with self.subTest(statement=statement):
                self.reject(MODULE+'program p\nuse boxes\ntype(box)::a\n'+statement+'\nend\n')

    def test_private_constructor_arguments(self):
        for value in ['box(8)','box(value=8)']:
            with self.subTest(value=value):
                self.reject(MODULE+'program p\nuse boxes\ntype(box)::a\na='+value+'\nend\n')

    def test_public_component_overrides_default(self):
        module=MODULE.replace('integer :: value = 3','integer :: value = 3\ninteger,public :: label = 4')
        self.check(module+'program p\nuse boxes\ntype(box)::a\na=box(label=12)\na%label=13\nprint *,a%get(),a%label\nend\n','3 13')

    def test_import_rename_and_reexport_do_not_expose_components(self):
        module=MODULE+'''module bridge
use boxes, only: renamed_box=>box
end module
'''
        self.reject(module+'program p\nuse bridge,only:local_box=>renamed_box\ntype(local_box)::a\nprint *,a%value\nend\n')

    def test_module_internal_procedure_has_private_access(self):
        module=MODULE.replace('end module','''subroutine host()
type(box)::x
x%value=10
call inner()
contains
subroutine inner()
print *,x%value
end subroutine
end subroutine
end module''')
        self.check(module+'program p\nuse boxes\ncall host()\nend\n','10')

    def test_external_contained_procedure_has_no_module_privilege(self):
        self.reject(MODULE+'''program p
use boxes
type(box)::x
call inner()
contains
subroutine inner()
print *,x%value
end subroutine
end
''')

    def test_nested_component_and_record_array_element(self):
        module=MODULE+'''module wrappers
use boxes
type wrapper
type(box)::item
end type
end module
'''
        for declaration,expr in [('type(wrapper)::x','x%item%value'),('type(box)::x(2)','x(1)%value')]:
            with self.subTest(expr=expr):
                self.reject(module+'program p\nuse wrappers\n'+declaration+'\nprint *,'+expr+'\nend\n')

    def test_private_array_component_sections(self):
        module='''module m
type t
private
integer::a(2)=[1,2]
end type
end module
'''
        for statement in ['print *,x%a(:)','x%a(1)=2']:
            with self.subTest(statement=statement):
                self.reject(module+'program p\nuse m\ntype(t)::x\n'+statement+'\nend\n')

    def test_inherited_private_field_keeps_defining_module(self):
        module=MODULE+'''module extensions
use boxes
type,extends(box)::child
integer::label=2
end type
end module
'''
        self.reject(module+'program p\nuse extensions\ntype(child)::x\nprint *,x%value\nend\n')
        self.reject(module+'program p\nuse extensions\ntype(child)::x\nprint *,x%box%value\nend\n')
        self.check(module+'program p\nuse extensions\ntype(child)::x\ncall x%set(6)\nprint *,x%get(),x%label\nend\n','6 2')

    def test_local_type_shadow_does_not_inherit_privacy(self):
        self.check(MODULE+'''program p
use boxes,only:box
call inner()
contains
subroutine inner()
type local
integer::value
end type
type(local)::a
a%value=11
print *,a%value
end subroutine
end
''','11')

    def test_private_requires_module_type(self):
        self.reject('program p\ntype t\nprivate\ninteger::x\nend type\nend\n','PRIVATE components require')

    def test_keyword_procedure_names_in_headers_calls_and_end_names(self):
        self.check("""module m
contains
subroutine print(n)
integer::n
print *,n
end subroutine print
subroutine read(n)
integer::n
print *,n+1
end subroutine read
end module
program p
use m
call print(4)
call read(4)
end
""",'4 5')

    def test_private_binding_default_with_public_overrides(self):
        module=MODULE.replace('procedure :: get\nprocedure :: set', 'private\nprocedure,public :: get\nprocedure :: set')
        self.check(module+'program p\nuse boxes\ntype(box)::x\nx=make(9)\nprint *,x%get()\nend\n','9')
        self.reject(module+'program p\nuse boxes\ntype(box)::x\ncall x%set(4)\nend\n','binding `set` is PRIVATE')

    def test_private_binding_attribute_and_public_generic(self):
        module=MODULE.replace('procedure :: get', 'procedure,private :: get\ngeneric,public :: value_of => get')
        self.check(module+'program p\nuse boxes\ntype(box)::x\nx=make(7)\nprint *,x%value_of()\nend\n','7')
        self.reject(module+'program p\nuse boxes\ntype(box)::x\nprint *,x%get()\nend\n','binding `get` is PRIVATE')

    def test_private_generic_is_not_exposed(self):
        module=MODULE.replace('procedure :: get', 'procedure :: get\ngeneric,private :: value_of => get')
        self.reject(module+'program p\nuse boxes\ntype(box)::x\nprint *,x%value_of()\nend\n','binding `value_of` is PRIVATE')

    def test_select_type_guard_keeps_component_privacy(self):
        module=MODULE+"module ext\nuse boxes\ntype,extends(box)::child\nprivate\ninteger::secret=2\nend type\nend module\n"
        self.reject(module+"""program p
use ext
type(child)::x
call show(x)
contains
subroutine show(a)
class(box)::a
select type(b=>a)
type is(child)
print *,b%secret
end select
end subroutine
end
""")

    def test_typeof_preserves_component_privacy(self):
        self.reject(MODULE+"program p\nuse boxes\ntype(box)::x\ntypeof(x)::y\nprint *,y%value\nend\n")

    def test_associate_preserves_component_privacy(self):
        self.reject(MODULE+"program p\nuse boxes\ntype(box)::x\nassociate(y=>x)\nprint *,y%value\nend associate\nend\n")

    def test_private_whole_object_io_requires_defining_module(self):
        for statement in ['print *,x','write(*,*)x','read(*,*)x','print "(i0)",x']:
            with self.subTest(statement=statement):
                self.reject(MODULE+'program p\nuse boxes\ntype(box)::x\n'+statement+'\nend\n')
        module=MODULE.replace('end module', 'subroutine show(x)\ntype(box)::x\nprint *,x\nend subroutine\nend module')
        self.check(module+'program p\nuse boxes\ntype(box)::x\ncall show(x)\nend\n','3')

    def test_module_file_retains_access_metadata(self):
        with tempfile.TemporaryDirectory(prefix='private-rmod-') as tmp:
            directory=Path(tmp);src=directory/'boxes.f90';src.write_text(MODULE)
            r=self.invoke([COMPILER,'-c',src],directory)
            self.assertEqual(r.returncode,0,r.stderr)
            main=directory/'main.f90'
            main.write_text('program p\nuse boxes\ntype(box)::x\nprint *,x%value\nend\n')
            r=self.invoke([COMPILER,'-c',main],directory)
            self.assertNotEqual(r.returncode,0)
            self.assertIn('is PRIVATE to module',r.stderr)
            self.assertIn('main.f90:4:',r.stderr)
            main.write_text('program p\nuse boxes\ntype(box)::x\nx=make(5)\nprint *,x%get()\nend\n')
            r=self.invoke([COMPILER,'-c',main],directory)
            self.assertEqual(r.returncode,0,r.stderr)


if __name__=='__main__':
    unittest.main()
