"""Scalar FINAL bindings, lifetime events and explicit unsupported cases."""
import unittest
import test_deallocate
import test_module_compilation
from test_record_read import ROOT

MODULE = """module m
implicit none
type :: item
 integer :: value=7
 integer,allocatable :: data(:)
contains
 final :: finish
end type
contains
subroutine finish(x)
type(item),intent(inout)::x
print *, 'final',x%value,allocated(x%data)
end subroutine
"""


class Finalization(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xscalar_final.f90').read_text(),
                   'final 1 T\nreset 0 F\nfinal 2 T\n3 9\nfinal 3 T\ndone\n')

    def test_defined_assignment_diagnosed(self):
        program=MODULE.replace('contains\nsubroutine finish', '''interface assignment(=)
module procedure assign_item
end interface
contains
subroutine assign_item(x,y)
type(item),intent(out)::x
type(item),intent(in)::y
x%value=y%value
end subroutine
subroutine finish''',1)
        self.reject(program+'end module\nend\n','defined assignment involving finalizable')

    def test_pure_finalizer(self):
        self.check('''module m
implicit none
type::item
integer::value=7
contains
final::finish
end type
contains
pure subroutine finish(x)
type(item),intent(inout)::x
x%value=0
end subroutine
pure subroutine work(n)
integer,intent(out)::n
type(item)::x
x=item(12)
n=x%value
end subroutine
end module
use m
implicit none
integer::n
call work(n)
print *,n
end
''','12\n')

    def test_nondefault_output_component_is_undefined(self):
        self.reject(MODULE.replace(' integer :: value=7',' integer :: unset\n integer :: value=7')+'''subroutine reset(x)
type(item),intent(out)::x
print *,x%unset
end subroutine
end module
use m
implicit none
type(item)::x
x%unset=42
call reset(x)
end
''','uninitialized')

    def test_scope_and_return(self):
        self.check(MODULE + """subroutine work(early)
logical,intent(in)::early
type(item)::x
x%value=11
allocate(x%data(1))
if(early) return
x%value=12
end subroutine
end module
use m
implicit none
type(item)::main_object
call work(.true.)
call work(.false.)
print *, 'done'
end
""", 'final 11 T\nfinal 12 T\ndone\n')

    def test_finalizer_deallocates_component(self):
        module=MODULE.replace("print *, 'final',x%value,allocated(x%data)",
                              "print *, 'final',x%value,allocated(x%data)\nif(allocated(x%data)) deallocate(x%data)")
        self.check(module+'''subroutine reset(x)
type(item),intent(out)::x
print *,allocated(x%data)
end subroutine
subroutine work()
type(item)::x
allocate(x%data(1))
call reset(x)
x=item(5,[2,3])
end subroutine
end module
use m
implicit none
call work()
end
''','final 7 T\nF\nfinal 7 F\nfinal 5 T\n')

    def test_assignment_rhs_and_synthetic_copies(self):
        program = MODULE + """integer function next(x)
integer,intent(in)::x
print *, 'rhs',x
next=x+1
end function
subroutine work()
type(item)::x
allocate(x%data(1))
x%data=3
x=item(next(x%value),[4,5])
print *,x%value,x%data
x=x
print *,x%value,x%data
end subroutine
end module
use m
implicit none
call work()
end
"""
        # The installed experimental gfortran elides finalization for x=x.
        self.check(program, 'rhs 7\nfinal 7 T\n8 4 5\nfinal 8 T\n8 4 5\nfinal 8 T\n',reference=False)
        self.check(program.replace('x=x\nprint *,x%value,x%data\n',''),
                   'rhs 7\nfinal 7 T\n8 4 5\nfinal 8 T\n')

    def test_intent_out_before_defaults_and_deallocation(self):
        self.check(MODULE + """subroutine reset(x)
type(item),intent(out)::x
print *,x%value,allocated(x%data)
x%value=13
end subroutine
subroutine work()
type(item)::x
x%value=9
allocate(x%data(2))
call reset(x)
call reset(x)
end subroutine
end module
use m
implicit none
call work()
end
""", 'final 9 T\n7 F\nfinal 13 F\n7 F\nfinal 13 F\n')

    def test_allocatable_scope_and_deallocate(self):
        self.check(MODULE + """subroutine work()
type(item),allocatable::x,y
allocate(x)
x%value=10
deallocate(x)
allocate(x)
x%value=20
allocate(y)
y%value=30
end subroutine
end module
use m
implicit none
call work()
end
""", 'final 10 F\nfinal 30 F\nfinal 20 F\n',reference=False)

    def test_saved_and_main_not_finalized(self):
        module=MODULE.replace(' integer,allocatable :: data(:)\n','').replace("print *, 'final',x%value,allocated(x%data)","print *, 'final',x%value")
        self.check(module + """subroutine work()
type(item),save::x
x%value=x%value+1
print *,x%value
end subroutine
end module
use m
implicit none
type(item)::x
call work()
call work()
stop
end
""", '8\n9\n')

    def test_nested_component_order(self):
        self.check("""module m
implicit none
type::child
integer::value=2
contains
final::end_child
end type
type::parent
type(child)::c
contains
final::end_parent
end type
contains
subroutine end_child(x)
type(child)::x
print *, 'child',x%value
end subroutine
subroutine end_parent(x)
type(parent)::x
print *, 'parent',x%c%value
x%c%value=3
end subroutine
subroutine work()
type(parent)::x
end subroutine
end module
use m
implicit none
call work()
end
""", 'parent 2\nchild 3\n')

    def test_block_exit_cycle_and_return(self):
        self.check(MODULE + """subroutine work()
integer::i
do i=1,3
 block
  type(item)::x
  x%value=i
  if(i==1) cycle
  if(i==2) exit
 end block
end do
block
 type(item)::x
 x%value=4
 return
end block
end subroutine
end module
use m
implicit none
call work()
end
""", 'final 1 F\nfinal 2 F\nfinal 4 F\n')

    def test_invalid_binding(self):
        for decl in ['type(item),optional::x','class(item)::x',
                     'type(item),intent(out)::x','type(item),allocatable::x',
                     'type(item),pointer::x']:
            with self.subTest(decl=decl):
                self.reject(MODULE.replace('type(item),intent(inout)::x',decl)
                            +'end module\nend\n', 'FINAL dummy')

    def test_unsupported_arrays_and_results(self):
        self.reject(MODULE+'end module\nuse m\ntype(item)::x(2)\nend\n',
                    'finalization of arrays')
        self.reject(MODULE+'''function make() result(x)
type(item)::x
end function
end module
end
''','function results of finalizable type')
        self.reject(MODULE+'end module\nuse m\ntype(item)::x\ntypeof(x)::y\nend\n',
                    'TYPEOF/CLASSOF of finalizable')
        self.reject(MODULE+'end module\nuse m\nclass(item),allocatable::x\nend\n',
                    'polymorphic objects')

    def test_optional_output(self):
        self.check(MODULE + """subroutine reset(x)
type(item),optional,intent(out)::x
if(.not.present(x)) return
print *,x%value,allocated(x%data)
end subroutine
end module
use m
implicit none
type(item)::x
x%value=9
call reset()
call reset(x)
end
""", 'final 9 F\n7 F\n')

    def test_separate_private_finalizer(self):
        module=MODULE.replace('implicit none','implicit none\nprivate\npublic::item',1)+'end module\n'
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',module)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','''use m,only:local_item=>item
implicit none
block
 type(local_item)::x
 x%value=17
end block
end
''')
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'final 17 F')

    def test_stop_does_not_finalize(self):
        self.check(MODULE + """subroutine work()
type(item)::x
print *, 'stop'
stop
end subroutine
end module
use m
implicit none
call work()
end
""", 'stop\n')

    def test_allocatable_assignment_and_move(self):
        self.check(MODULE+'''end module
use m
implicit none
type(item),allocatable::x,y
x=item(3,[1,2])
x=item(4,[5])
allocate(y)
y%value=9
call move_alloc(from=x,to=y)
print *,allocated(x),y%value,y%data
deallocate(y)
end
''', 'final 3 T\nfinal 9 F\nF 4 5\nfinal 4 T\n')

    def test_noncolon_binding_and_no_intent(self):
        self.check(MODULE.replace('final :: finish','final finish').replace(
            'type(item),intent(inout)::x','type(item)::x')+'''subroutine work()
type(item)::x
end subroutine
end module
use m
call work()
end
''', 'final 7 F\n')

    def test_other_invalid_bindings(self):
        self.reject(MODULE.replace('final :: finish','final :: finish,finish')+
                    'end module\nend\n','duplicate FINAL')
        self.reject(MODULE.replace('subroutine finish(x)','subroutine finish(x,y)').replace(
            'type(item),intent(inout)::x','type(item),intent(inout)::x,y')+
                    'end module\nend\n','exactly one dummy')
        self.reject(MODULE.replace('type(item),intent(inout)::x','type(item),intent(inout)::x(:)')+
                    'end module\nend\n','array and ELEMENTAL FINAL')

    def test_finalized_component_assignment(self):
        self.check("""module m
implicit none
type::child
integer::value=2
contains
final::end_child
end type
type::parent
type(child)::c
end type
contains
subroutine end_child(x)
type(child)::x
print *,x%value
end subroutine
subroutine work()
type(parent)::x
x%c=child(8)
end subroutine
end module
use m
implicit none
call work()
end
""", '2\n8\n')


if __name__=='__main__': unittest.main()
