"""Scalar record function results preserve borrowed static target identities."""
import unittest
import subprocess
from test_record_read import ROOT, COMPILER
from test_polymorphic_pointer_saved_owners import TYPES, MAIN
import test_polymorphic_pointers as shared

class PointerOwnerResults(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check
    reject = shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_result.f90').read_text(),
                   '14 14\nT\nF 14')

    def test_factory_alias_and_dispatch(self):
        self.check(TYPES + '''contains
function make_entry() result(x)
type(entry)::x
x%object=>target
end function
end module
''' + MAIN + '''copy=make_entry()
call copy%object%add(3)
current=make_entry()
print *,current%object%get(),associated(copy%object,current%object)
nullify(current%object)
print *,associated(current%object),copy%object%get()
end
''', '14 T\nF 14')

    def test_implicit_result_saved_target(self):
        self.check(TYPES + '''contains
function make_entry()
type(entry)::make_entry
type(child),target,save::local
local%value=local%value+2
make_entry%object=>local
end function
end module
''' + MAIN + '''copy=make_entry()
current=make_entry()
print *,copy%object%get(),current%object%get(),associated(copy%object,current%object)
end
''', '15 15 T')

    def test_null_and_early_return(self):
        self.check(TYPES + '''contains
function make_entry(attach) result(x)
logical,intent(in)::attach
type(entry)::x
if(.not.attach)return
x%object=>target
end function
end module
''' + MAIN + '''copy=make_entry(.true.)
current=make_entry(.false.)
print *,associated(copy%object),associated(current%object),copy%object%get()
copy=make_entry(.false.)
print *,associated(copy%object)
end
''', 'T F 11\nF')

    def test_owner_dummy_and_nested_factory(self):
        self.check(TYPES + '''contains
function duplicate(src) result(x)
type(entry),intent(in)::src
type(entry)::x
x=src
end function
function make_entry() result(x)
type(entry)::x,tmp
tmp%object=>target
x=duplicate(tmp)
end function
end module
''' + MAIN + '''copy=make_entry()
current=duplicate(copy)
call current%object%add(2)
print *,copy%object%get(),associated(current%object,copy%object)
end
''', '13 T')

    def test_borrowed_return_diagnosed(self):
        self.check(TYPES + '''contains
function make_entry() result(x)
type(entry)::x
type(child),target::local
x%object=>local
end function
end module
''' + MAIN + 'copy=make_entry()\nend\n',
                   error='pointer component function result currently requires a saved or module target', reference=False)

    def test_early_borrowed_return_diagnosed(self):
        self.check(TYPES + '''contains
function make_entry(early) result(x)
logical,intent(in)::early
type(entry)::x
type(child),target::local
x%object=>local
if(early)return
nullify(x%object)
end function
end module
''' + MAIN + 'copy=make_entry(.true.)\nend\n',
                   error='pointer component function result currently requires a saved or module target', reference=False)

    def test_temporary_borrow_cleared(self):
        self.check(TYPES + '''contains
function make_entry() result(x)
type(entry)::x
type(child),target::local
x%object=>local
nullify(x%object)
end function
end module
''' + MAIN + 'copy=make_entry()\nprint *,associated(copy%object)\nend\n', 'F')

    def test_borrowed_owner_dummy_return_diagnosed(self):
        self.check(TYPES + '''contains
function duplicate(src) result(x)
type(entry),intent(in)::src
type(entry)::x
x=src
end function
end module
''' + MAIN + '''type(child),target::local
copy%object=>local
current=duplicate(copy)
end
''', error='pointer component function result currently requires a saved or module target', reference=False)

    def test_mixed_environment_returns_static_target(self):
        self.check(TYPES + '''contains
function make_entry(use_saved) result(x)
logical,intent(in)::use_saved
type(entry)::x
type(child),target::local
if(use_saved)then
x%object=>target
else
x%object=>local
end if
end function
end module
''' + MAIN + 'copy=make_entry(.true.)\nprint *,copy%object%get()\nend\n', '11')

    def test_dynamic_type_and_distinct_static_identities(self):
        self.check(TYPES + '''type(base),target::first
type(child),target::second
contains
function make_entry(which) result(x)
integer,intent(in)::which
type(entry)::x
select case(which)
case(1)
x%object=>first
case(2)
x%object=>second
case default
x%object=>target
end select
end function
end module
''' + MAIN + '''type(entry)::third
copy=make_entry(1)
current=make_entry(2)
third=make_entry(3)
call current%object%add(4)
print *,copy%object%get(),current%object%get(),third%object%get()
print *,associated(current%object,third%object),associated(copy%object,first)
end
''', '1 15 11\nF T')

    def test_nested_owner_result(self):
        self.check(TYPES + '''type wrapper
type(entry)::item
end type
contains
function make_entry() result(x)
type(wrapper)::x
x%item%object=>target
end function
end module
program p
use owners
implicit none
type(wrapper)::x,y
x=make_entry()
y=x
nullify(x%item%object)
print *,associated(x%item%object),y%item%object%get()
end
''', 'F 11')

    def test_result_as_owner_actual(self):
        self.check(TYPES + '''contains
function make_entry() result(x)
type(entry)::x
x%object=>target
end function
subroutine show(x)
type(entry),intent(in)::x
print *,x%object%get()
end subroutine
end module
''' + MAIN + 'call show(make_entry())\nend\n', '11')

    def test_uninitialized_target_read(self):
        source = '''module m
implicit none
type node
integer::value
end type
type entry
class(node),pointer::object=>null()
end type
type(node),target::t
contains
function make_entry() result(x)
type(entry)::x
x%object=>t
end function
end module
program p
use m
implicit none
type(entry)::x
x=make_entry()
print *,x%object%value
end
'''
        self.check(source, error='read of uninitialized variable', reference=False)
        self.check(source.replace('print *,x%object%value', 'x%object%value=7\nprint *,x%object%value'), '7')

    def test_array_result_remains_diagnosed(self):
        self.reject(TYPES + '''contains
function make_entry() result(x)
type(entry)::x(2)
end function
end module
''' + MAIN + 'end\n', 'array function results with polymorphic POINTER components')

    def test_private_module_target_separate_compilation(self):
        source = TYPES.replace('type(child),target::target', 'type(child),target,private::target') + '''contains
function make_entry() result(x)
type(entry)::x
x%object=>target
end function
end module
''' + MAIN + '''copy=make_entry()
call copy%object%add(4)
print *,copy%object%get()
end
'''
        self.check(source, '15')
        module, main = source.split('program p\n')
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text('program p\n'+main)
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([self.work/'main.exe'], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), ['15'])

    def test_saved_factory_target_separate_compilation(self):
        module = TYPES + '''contains
function make_entry() result(x)
type(entry)::x
type(child),target,save::local
local%value=local%value+1
x%object=>local
end function
end module
'''
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text(MAIN + '''copy=make_entry()
current=make_entry()
print *,copy%object%get(),associated(copy%object,current%object)
end
''')
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([self.work/'main.exe'], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), ['13', 'T'])

    def test_separate_compilation_with_another_target_module(self):
        module = TYPES + '''contains
function make_entry() result(x)
type(entry)::x
x%object=>target
end function
end module
'''
        (self.work/'m.f90').write_text(module)
        (self.work/'other.f90').write_text('''module another_target
use owners,only:child
implicit none
type(child),target::other
end module
''')
        (self.work/'main.f90').write_text(MAIN.replace('use owners', 'use owners\nuse another_target') + '''copy=make_entry()
target%value=6
other%value=90
print *,copy%object%get(),associated(copy%object,target),associated(copy%object,other)
end
''')
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'-c','other.f90','-o','other.obj'],
                        [COMPILER,'main.f90','m.obj','other.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([self.work/'main.exe'], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), ['16', 'T', 'F'])

if __name__ == '__main__':
    unittest.main()
