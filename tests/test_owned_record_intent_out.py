"""INTENT(OUT) entry cleanup for arrays with owned components."""
import unittest

import test_deallocate
import test_module_compilation
from test_record_read import ROOT


TYPES = """module m
implicit none
type :: child
 integer :: value=7
 integer, allocatable :: data(:)
end type
type :: row
 type(child) :: nested
 character(len=3), allocatable :: labels(:)
 real :: fixed(2)=[1.0,2.0]
 integer :: unset
end type
contains
"""


class OwnedRecordIntentOut(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xowned_record_intent_out.f90').read_text(),
                   'F 7\nF 7\nF 7\nF 7\n-1 0\n1 10 2 20\n')

    def test_separately_compiled_module(self):
        module, main = (ROOT/'tests/programs/xowned_record_intent_out.f90').read_text().split('program demo', 1)
        helper = test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        for opt in ['-O0', '-O2']:
            with self.subTest(opt=opt):
                source = helper.source('owners', module)
                helper.success(opt, '-c', source)
                source.unlink()
                source = helper.source('main', 'program demo' + main)
                helper.success(opt, '-c', source)
                helper.run_objects(['owners', 'main'],
                                   'F 7 F 7 F 7 F 7 -1 0 1 10 2 20')

    def test_repeated_calls_defaults_and_bounds(self):
        self.check(TYPES + """subroutine reset(a)
type(row),intent(out)::a(:)
integer::i
print *,lbound(a,1),ubound(a,1)
do i=lbound(a,1),ubound(a,1)
 print *,allocated(a(i)%nested%data),allocated(a(i)%labels)
 print *,a(i)%nested%value,a(i)%fixed
 allocate(a(i)%nested%data(-2:0),a(i)%labels(1))
 a(i)%nested%data=[3,4,5]
 a(i)%labels='new'
 a(i)%nested%value=99
 a(i)%fixed=8.0
end do
end subroutine
end module
use m
implicit none
type(row)::a(-3:-2)
allocate(a(-3)%nested%data(0),a(-2)%labels(2))
call reset(a)
call reset(a)
print *,lbound(a,1),ubound(a,1)
print *,lbound(a(-3)%nested%data,1),a(-3)%nested%data
print *,a(-2)%labels
end
""", ("1 2\nF F\n7 1.0 2.0\nF F\n7 1.0 2.0\n" * 2)
           + "-3 -2\n-2 3 4 5\nnew\n")

    def test_rank_two_section_and_empty(self):
        program = """module m
implicit none
type :: row
 integer::value=7
 integer,allocatable::data(:)
end type
contains
subroutine reset(a)
type(row),intent(out)::a(:,:)
integer::i,j
do j=1,size(a,2)
 do i=1,size(a,1)
  if(allocated(a(i,j)%data)) stop 1
  if(a(i,j)%value/=7) stop 2
  allocate(a(i,j)%data(1))
  a(i,j)%data=9
 end do
end do
end subroutine
end module
use m
implicit none
type(row)::a(3,2),empty(0,2)
integer::i,j
do j=1,2
 do i=1,3
  allocate(a(i,j)%data(1))
  a(i,j)%data=i+10*j
 end do
end do
call reset(a(1:3:2,:))
call reset(empty)
print *,a(1,1)%data,a(2,1)%data,a(3,2)%data
end
"""
        # GCC 17.0.0 20260510 deallocates the unselected middle row on
        # entry too. Check preservation independently of that reference.
        self.check(program, "9 12 9\n", reference=False)
        self.check(program.replace('a(1:3:2,:)', 'a(:,:)'), "9 9 9\n")

    def test_allocatable_output_and_early_return(self):
        self.check(TYPES + """subroutine reset(a,make)
type(row),allocatable,intent(out)::a(:)
logical,intent(in)::make
print *,allocated(a)
if(.not.make) return
allocate(a(-2:-1))
print *,a(-2)%nested%value,a(-1)%fixed
allocate(a(-2)%nested%data(1),a(-1)%labels(1))
a(-2)%nested%data=8
a(-1)%labels='yes'
end subroutine
end module
use m
implicit none
type(row),allocatable::a(:)
call reset(a,.true.)
print *,lbound(a,1),ubound(a,1)
call reset(a,.false.)
print *,allocated(a)
call reset(a,.true.)
print *,a(-2)%nested%data,a(-1)%labels
end
""", "F\n7 1.0 2.0\n-2 -1\nF\nF\nF\n7 1.0 2.0\n8 yes\n")

    def test_optional_absent_and_present(self):
        self.check(TYPES + """subroutine reset(a)
type(row),optional,intent(out)::a(:)
if(.not.present(a)) return
print *,allocated(a(1)%nested%data),a(1)%nested%value
end subroutine
end module
use m
implicit none
type(row)::a(1)
allocate(a(1)%nested%data(2))
call reset()
print *,allocated(a(1)%nested%data)
call reset(a)
call reset(a)
end
""", "T\nF 7\nF 7\n")

    def test_ordinary_components_remain_undefined(self):
        self.reject(TYPES + """subroutine reset(a)
type(row),intent(out)::a(:)
print *,a(1)%unset
end subroutine
end module
use m
implicit none
type(row)::a(1)
a(1)%unset=42
call reset(a)
end
""", "uninitialized")

    def test_forwarded_pure_and_explicit_shape(self):
        self.check("""module m
implicit none
type :: row
 logical::ready=.true.
 real,allocatable::data(:,:,:)
end type
contains
pure subroutine reset(a,n)
integer,intent(in)::n
type(row),intent(out)::a(n)
integer::i
do i=1,n
 allocate(a(i)%data(1,2,1))
 a(i)%data=3.0
end do
end subroutine
subroutine forward(a)
type(row),intent(inout)::a(:)
call reset(a,size(a))
end subroutine
end module
use m
implicit none
type(row),allocatable::a(:)
allocate(a(-2:-1))
allocate(a(-2)%data(0,1,1))
call forward(a)
call forward(a)
print *,lbound(a,1),ubound(a,1),a(-2)%ready,sum(a(-1)%data)
end
""", "-2 -1 T 6.0\n")

    def test_rank_fifteen(self):
        axes=','.join(['1']*15)
        deferred=','.join([':']*15)
        self.check(f"""module m
implicit none
type :: row
 integer::value=7
 integer,allocatable::data(:)
end type
contains
subroutine reset(a)
type(row),intent(out)::a({deferred})
print *,allocated(a({axes})%data),a({axes})%value
allocate(a({axes})%data(1))
a({axes})%data=9
end subroutine
end module
use m
implicit none
type(row)::a({axes})
allocate(a({axes})%data(2))
call reset(a)
call reset(a)
print *,a({axes})%data
end
""", "F 7\nF 7\n9\n")

    def test_optional_allocatable_runtime_shape(self):
        self.check("""module m
implicit none
type :: row
 integer::value=7
 logical,allocatable::flags(:)
end type
contains
subroutine reset(n,a)
integer,intent(in)::n
type(row),optional,allocatable,intent(out)::a(:,:)
if(.not.present(a)) return
if(allocated(a)) stop 1
allocate(a(n,2))
if(n==0) return
print *,a(1,1)%value,allocated(a(1,1)%flags)
allocate(a(1,1)%flags(1))
a(1,1)%flags=.true.
end subroutine
end module
use m
implicit none
type(row),allocatable::a(:,:)
integer::n
n=2
call reset(n)
call reset(n,a)
print *,a(1,1)%flags
call reset(0,a)
print *,allocated(a),shape(a)
call reset(n,a)
print *,a(1,1)%flags
end
""", "7 F\nT\nT 0 2\n7 F\nT\n")

    def test_pointer_output_does_not_reset_target(self):
        self.check("""module m
implicit none
type :: row
 integer::value=7
end type
contains
subroutine bind(p,a)
type(row),pointer,intent(out)::p(:)
type(row),target,intent(inout)::a(:)
p=>a
end subroutine
end module
use m
implicit none
type(row),target::a(2)
type(row),pointer::p(:)
a%value=[11,12]
p=>a
call bind(p,a)
print *,p%value
print *,a%value
end
""", "11 12\n11 12\n")


if __name__ == '__main__':
    unittest.main()
