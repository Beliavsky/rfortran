"""Opt-in module accessibility policy and source accessibility advice."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER


class ModuleDefaultPrivate(unittest.TestCase):
    def command(self, args, directory, error=False):
        result=subprocess.run([str(COMPILER),*map(str,args)],cwd=directory,
            capture_output=True,text=True,encoding='utf-8',timeout=60)
        self.assertEqual(result.returncode!=0,error,result.stdout+result.stderr)
        return result

    def check(self, source, flags=(), error=False):
        with tempfile.TemporaryDirectory(prefix='rfort-default-private-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(source)
            result=self.command([*flags,'check',path],tmp,error)
            self.assertEqual(path.read_text(),source)
            return result.stderr

    def execute(self, source, flags=('--default-private',)):
        with tempfile.TemporaryDirectory(prefix='rfort-default-private-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(source)
            for args in [list(flags)+['run',path],
                         list(flags)+['-O0',path,'--run','-o',Path(tmp)/'zero.exe'],
                         list(flags)+['-O2',path,'--run','-o',Path(tmp)/'two.exe']]:
                result=self.command(args,tmp)
                self.assertEqual(result.stdout.split(),['PASS'])
            self.assertEqual(path.read_text(),source)

    def test_standard_default_and_implicit_private(self):
        source='''module m
integer,parameter::value=7
end module
program p
use m,only:value
print *,value
end
'''
        self.assertEqual(self.check(source),'')
        self.assertIn('not a public entity',self.check(source,['--default-private'],error=True))
        # The option is global and also works after the subcommand.
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            self.assertIn('not a public entity', self.command(
                ['check',path,'--default-private'],tmp,error=True).stderr)

    def test_explicit_exports_and_private_implementation(self):
        self.execute('''module m
implicit none
integer,parameter::hidden=3
integer,public,parameter::published=7
type,public::box
 integer::value
end type
interface add
 module procedure add_impl
end interface
public::add,answer
contains
integer function add_impl(a,b) result(c)
integer,intent(in)::a,b
c=a+b
end function
integer function answer() result(n)
n=hidden+published
end function
end module
program p
use m,only:published,box,add,answer
type(box)::x
x%value=published
if(add(x%value,answer())/=17) stop 1
print *,"PASS"
end
''')

    def test_each_entity_kind_is_private(self):
        module='''module m
integer::variable
integer,parameter::constant=3
type::item
 integer::value
end type
interface generic
 module procedure implementation
end interface
contains
integer function implementation(i)
integer,intent(in)::i
implementation=i
end function
end module
'''
        for entity in ['variable','constant','item','generic','implementation']:
            with self.subTest(entity=entity):
                source=module+f'program p\nuse m,only:{entity}\nend\n'
                self.assertIn('not a public entity',self.check(source,['--default-private'],error=True))

    def test_explicit_operator_export(self):
        source='''module m
type,public::number
 integer::value
end type
interface operator(+)
 module procedure add
end interface
public::operator(+)
contains
function add(a,b) result(c)
type(number),intent(in)::a,b
type(number)::c
c%value=a%value+b%value
end function
end module
program p
use m,only:number,operator(+)
type(number)::a,b,c
a%value=3
b%value=4
c=a+b
if(c%value/=7) stop 1
print *,"PASS"
end
'''
        self.execute(source)
        self.check(source.replace('public::operator(+)\n',''),['--default-private'],error=True)

    def test_standalone_public_and_explicit_private(self):
        source='''module m
public
integer,parameter::value=7
integer,private::secret
end module
program p
use m,only:value
if(value/=7) stop 1
print *,"PASS"
end
'''
        self.execute(source)
        self.assertIn('not a public entity',self.check(
            source.replace('only:value','only:secret'),['--default-private'],error=True))
        self.assertIn('not a public entity',self.check(
            source.replace('public\n','private\n'),['--default-private'],error=True))

    def test_reexports_and_intrinsic_modules(self):
        self.execute('''module base
public
integer,parameter::value=7
end module
module wrapper
use base,only:value
public::value
end module
program p
use wrapper,only:value
use iso_fortran_env,only:int64
integer(int64)::x
x=value
if(x/=7) stop 1
print *,"PASS"
end
''')
        self.assertIn('not a public entity',self.check('''module base
public
integer,parameter::value=7
end module
module wrapper
use base,only:value
end module
program p
use wrapper,only:value
end
''',['--default-private'],error=True))

    def test_separate_compilation_preserves_producer_policy(self):
        with tempfile.TemporaryDirectory(prefix='rfort-default-private-') as tmp:
            root=Path(tmp);mods=root/'mods';mods.mkdir()
            module=root/'m.f90'
            module.write_text('''module m
integer,parameter::secret=3
public::answer
contains
integer function answer() result(n)
n=secret+4
end function
end module
''')
            obj=root/'m.obj'
            self.command(['--default-private','-c',module,'-J',mods,'-o',obj],tmp)
            # The interface records effective policy without editing the source.
            data=json.loads((mods/'m.rmod').read_text())
            self.assertIn('Accessibility',json.dumps(data['definition']))
            main=root/'main.f90'
            main.write_text('''use m,only:answer
if(answer()/=7) stop 1
print *,"PASS"
end
''')
            for flags in [[],['--default-private']]:
                run=self.command([*flags,main,obj,'-I',mods,'--run','-o',root/'main.exe'],tmp)
                self.assertEqual(run.stdout.split(),['PASS'])
            main.write_text('use m,only:secret\nprint *,secret\nend\n')
            self.assertIn('not a public entity',self.command(['-I',mods,'check',main],tmp,error=True).stderr)
            # A consumer's private-default option must not rewrite an already
            # compiled module that used the standard public default.
            module.write_text('module m\ninteger,parameter::secret=3\nend module\n')
            self.command(['-c',module,'-J',mods,'-o',obj],tmp)
            self.assertEqual(self.command(['--default-private','-I',mods,'check',main],tmp).stderr,'')

    def test_warning_controls_and_source_location(self):
        source='module example_mod\ninteger,public::value\nend module\n'
        self.assertEqual(self.check(source),'')
        self.assertNotIn('[module-default-public]',self.check(source,['-Wall']))
        warning=self.check(source,['-Wmodule-default-public'])
        self.assertEqual(warning.count('[module-default-public]'),1)
        self.assertIn('example.f90:1:8',warning)
        self.assertIn('module `example_mod`',warning)
        for flags in [
            ['-Wmodule-default-public','-Wno-module-default-public'],
            ['-Wno-module-default-public'],
        ]:
            self.assertEqual(self.check(source,flags),'')
        self.assertIn('[module-default-public]',self.check(source,
            ['-Wno-module-default-public','-Wmodule-default-public']))
        for flags in [['-Werror=module-default-public'],['-Wmodule-default-public','-Werror']]:
            self.assertIn('error: module',self.check(source,flags,error=True))
        self.assertEqual(self.check('!#allow(module-default-public)\n'+source,['-Wmodule-default-public']),'')
        self.assertIn('[module-default-public]',self.check(
            '!#deny(module-default-public)\n'+source,error=True))
        self.assertIn('[module-default-public]',self.check(source,
            ['--default-private','-Wmodule-default-public']))

    def test_explicit_default_suppresses_warning(self):
        for default in ['public','private']:
            source=f'module m\n{default}\ninteger::x\nend module\n'
            self.assertEqual(self.check(source,['-Wmodule-default-public']),'')

    def test_component_access_is_not_a_module_default(self):
        source='''module m
type,public::item
 private
 integer::value
end type
end module
'''
        self.assertEqual(self.check(source,['-Wmodule-default-public']).count('[module-default-public]'),1)

    def test_unused_private_warning_uses_effective_access(self):
        source='module m\ninteger::hidden\nend module\n'
        self.assertNotIn('[unused-variable]',self.check(source,['-Wunused-variable']))
        self.assertIn('[unused-variable]',self.check(source,['--default-private','-Wunused-variable']))

    def test_imported_modules_are_not_warned_about(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);module=root/'m.f90';main=root/'main.f90'
            module.write_text('module m\ninteger,parameter::value=3\nend module\n')
            self.command(['-c',module,'-o',root/'m.obj'],tmp)
            main.write_text('use m,only:value\nprint *,value\nend\n')
            self.assertEqual(self.command(['-Wmodule-default-public','check',main],tmp).stderr,'')


if __name__=='__main__':
    unittest.main()
