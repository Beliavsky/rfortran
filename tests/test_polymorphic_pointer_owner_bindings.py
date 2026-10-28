"""Type-bound and generic interfaces for borrowed pointer-component owners."""
import subprocess
import unittest
import test_polymorphic_pointers as shared
from test_record_read import ROOT, COMPILER

SOURCE=(ROOT/'tests/programs/xpolymorphic_pointer_owner_bindings.f90').read_text()
MODULE=SOURCE.split('program p\n')[0]
EXPECTED='13 3\n117 117 117 6\n117\nF 100 7\n-1 3'

class PointerOwnerBindings(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check
    reject=shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check(SOURCE,EXPECTED)

    def test_indexed_and_nested_receivers(self):
        self.check(MODULE+'''program p
use owner_entries
implicit none
type wrapper
type(entry)::part
end type
type(entry)::list(2)
type(wrapper)::w
type(child),target::a,b
call list(2)%set(target=a)
call list(2)%set(n=8)
print *,list(2)%get(),associated(list(1)%object)
call w%part%rebind(target=b)
call w%part%add(n=4)
call w%part%show()
print *,a%value,b%value
end
''','18 F\n15\n8 5')

    def test_generic_alias_and_specific_only_import(self):
        self.check(MODULE+'''program p
use owner_entries,only:entry,child,query=>fetch,entry_rebind
implicit none
type(entry)::x
type(child),target::a
call entry_rebind(x,a)
print *,query(x),query(4)
end
''','11 8')

    def test_separate_module_interface(self):
        (self.work/'m.f90').write_text(MODULE)
        (self.work/'main.f90').write_text('program p\n'+SOURCE.split('program p\n')[1])
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result=subprocess.run(command,cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
        result=subprocess.run([self.work/'main.exe'],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),EXPECTED.split())

    def test_generic_no_match(self):
        self.reject(MODULE+'''program p
use owner_entries
implicit none
type(entry)::x
call x%set(n='wrong')
end
''','no matching specific procedure for generic')

    def test_readonly_receiver_cannot_clear(self):
        self.reject(MODULE+'''program p
use owner_entries
implicit none
type(entry)::x
call bad(x)
contains
subroutine bad(y)
type(entry),intent(in)::y
call y%clear()
end subroutine
end
''','INTENT(IN)')

    def test_pure_generic_owner_function(self):
        source=MODULE.replace('integer function fetch_entry(x)', 'pure integer function fetch_entry(x)')
        source=source.replace('n=x%get()','n=x%object%value')
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x
type(child),target::a
call x%rebind(a)
print *,fetch(x)
end
''','1')

    def test_private_specific_through_public_generic(self):
        source=MODULE.replace('procedure::set_value=>entry_set_value',
                              'procedure,private::set_value=>entry_set_value')
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x
type(child),target::a
call x%set(target=a)
call x%set(n=9)
print *,x%get()
end
''','19')
        self.reject(source+'''program p
use owner_entries
implicit none
type(entry)::x
call x%set_value(9)
end
''','PRIVATE')

    def test_optional_keyword_and_two_owner_arguments(self):
        source=MODULE.replace('procedure::show=>entry_show',
                              'procedure::show=>entry_show\nprocedure::adjust\nprocedure::score')
        source=source[:source.rfind('end module\n')]+'''subroutine adjust(self,n)
class(entry),intent(in)::self
integer,intent(in),optional::n
if(present(n)) call self%object%add(n)
end subroutine
integer function together(x,y) result(n)
class(entry),intent(in)::x,y
n=x%get()+y%get()
end function
integer function score(self) result(n)
class(entry),intent(in)::self
n=self%get()
end function
end module
'''
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x
type(entry)::list(1)
type(special_entry)::y
type(child),target::a,b,c
call x%rebind(a)
call y%rebind(b)
call x%adjust()
call y%adjust(n=3)
print *,together(y=y,x=x),a%value,b%value
call list(1)%rebind(c)
call list(1)%adjust(n=2)
print *,list(1)%score(),c%value
end
''','125 1 4\n13 3')

    def test_named_kind_overloads(self):
        source=MODULE.replace('module owner_entries\nuse pointer_shapes\nimplicit none',
                              'module owner_entries\nuse pointer_shapes\nimplicit none\ninteger,parameter::ik=selected_int_kind(12),dp=kind(1.0d0)')
        source=source.replace('procedure::show=>entry_show',
                              'procedure::show=>entry_show\nprocedure::put_integer,put_real\ngeneric::put=>put_integer,put_real')
        source=source[:source.rfind('end module\n')]+'''subroutine put_integer(self,n)
class(entry),intent(in)::self
integer(kind=ik),intent(in)::n
self%object%value=int(n)
end subroutine
subroutine put_real(self,n)
class(entry),intent(in)::self
real(kind=dp),intent(in)::n
self%object%value=int(n)
end subroutine
end module
'''
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x
type(child),target::a
call x%rebind(a)
call x%put(7_ik)
print *,x%get()
call x%put(n=9.0_dp)
print *,x%get()
end
''','17\n19')

    def test_runtime_polymorphic_owner_is_explicit(self):
        self.reject(MODULE+'''program p
use owner_entries
implicit none
class(entry),allocatable::x
allocate(special_entry::x)
call x%show()
end
''','runtime polymorphic owner')

    def test_inherited_generic_retains_binding_identity(self):
        source=MODULE.replace('procedure::get=>entry_get',
                              'procedure::get=>entry_get\nprocedure::other=>entry_get\ngeneric::choose=>other')
        source=source.replace('procedure::get=>special_get',
                              'procedure::get=>special_get\ngeneric::inherited=>add')
        self.check(source+'''program p
use owner_entries
implicit none
type(special_entry)::x
type(child),target::a
call x%rebind(a)
print *,x%value(),x%choose()
call x%inherited(n=2)
print *,x%get(),a%value
end
''','111 11\n113 3')

    def test_rank_overloads(self):
        source=MODULE.replace('procedure::show=>entry_show',
                              'procedure::show=>entry_show\nprocedure::apply_scalar,apply_vector\ngeneric::apply=>apply_scalar,apply_vector')
        source=source.replace('module owner_entries\nuse pointer_shapes\nimplicit none',
                              'module owner_entries\nuse pointer_shapes\nimplicit none\ninteger,parameter::ik=kind(1)')
        source=source[:source.rfind('end module\n')]+'''subroutine apply_scalar(self,n)
class(entry),intent(in)::self
integer,intent(in)::n
self%object%value=n
end subroutine
subroutine apply_vector(self,n)
class(entry),intent(in)::self
integer,intent(in)::n(:)
self%object%value=sum(n)
end subroutine
end module
'''
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x
type(child),target::a
call x%rebind(a)
call x%apply(2)
print *,x%get()
call x%apply(n=[integer(kind=ik) :: 1,2,3])
print *,x%get()
end
''','12\n16')

    def test_pure_polymorphic_output_remains_invalid(self):
        self.reject(SOURCE.replace('subroutine entry_clear(self)',
                                   'pure subroutine entry_clear(self)'),
                    'PURE procedure cannot have a polymorphic INTENT(OUT) dummy')

    def test_declared_type_selects_generic_dynamic_type_selects_binding(self):
        source=MODULE.replace('procedure::show=>entry_show',
                              'procedure::show=>entry_show\nprocedure::inspect')
        source=source.replace('interface fetch\n','interface which\nmodule procedure which_entry,which_special\nend interface\ninterface fetch\n')
        source=source[:source.rfind('end module\n')]+'''subroutine inspect(self)
class(entry),intent(in)::self
print *,which(self),self%get()
end subroutine
integer function which_entry(x) result(n)
type(entry),intent(in)::x
n=1
end function
integer function which_special(x) result(n)
type(special_entry),intent(in)::x
n=2
end function
end module
'''
        self.check(source+'''program p
use owner_entries
implicit none
type(special_entry)::x
type(child),target::a
call x%rebind(a)
print *,which(x)
call x%inspect()
end
''','2\n1 111')

    def test_generic_preserves_intrinsic_fallback(self):
        source=MODULE.replace('interface fetch\n','interface max\nmodule procedure max_entry\nend interface\ninterface fetch\n')
        source=source[:source.rfind('end module\n')]+'''integer function max_entry(x,y) result(n)
type(entry),intent(in)::x,y
n=max(x%get(),y%get())
end function
end module
'''
        self.check(source+'''program p
use owner_entries
implicit none
type(entry)::x,y
type(child),target::a,b
call x%rebind(a)
call y%rebind(b)
call x%set(n=7)
print *,max(x,y),max(2,3)
end
''','17 3')

    def test_local_array_shadows_host_generic(self):
        self.check(MODULE+'''program p
use owner_entries
implicit none
call probe()
contains
subroutine probe()
type(entry)::fetch(1)
type(child),target::a
call fetch(1)%rebind(a)
print *,fetch(1)%get()
end subroutine
end
''','11')

if __name__=='__main__':unittest.main()
