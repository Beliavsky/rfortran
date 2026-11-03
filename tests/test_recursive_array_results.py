"""Recursive numeric array results: interpreter, native O0/O2 and gfortran."""
import unittest
from test_record_read import ROOT
import test_deallocate


class RecursiveArrayResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_automatic_locals_in_recursive_frames(self):
        self.check('''module m
implicit none
contains
recursive subroutine accumulate(a,n,depth)
integer,intent(in) :: n,depth
integer,intent(inout) :: a(n)
integer :: saved(0:n-1)
saved=depth
if(depth>0) call accumulate(a,n,depth-1)
a=a+saved
end subroutine
end module
program main
use m
implicit none
integer :: a(3),b(0)
a=0
call accumulate(a,3,3)
call accumulate(b,0,2)
print *,a,size(b)
end
''', '6 6 6 0\n')

    def test_fold_bounds_reproducer(self):
        self.check((ROOT / 'tests/programs/mfve_fold_bounds.f90').read_text()
                   + '\n' + (ROOT / 'tests/programs/xfve_fold_bounds.f90').read_text(),
                   '4 3 3\n')

    def test_real_result_reproducer(self):
        self.check((ROOT / 'tests/programs/xrecursive_real_array_result.f90').read_text(),
                   '1.0 2.0 3.0\n')

    def test_runtime_double_matrix_and_multiple_frames(self):
        self.check('''module m
implicit none
contains
recursive function accumulate(x,n) result(y)
real(kind=kind(1.d0)),intent(in)::x(:,:)
integer,intent(in)::n
real(kind=kind(1.d0))::y(size(x,1),size(x,2))
if(n==0)then
   y=x
else
   y=accumulate(x,n-1)+real(n,kind=kind(1.d0))
end if
end function
end module
program main
use m
implicit none
real(kind=kind(1.d0)),allocatable::x(:,:),y(:,:)
integer::n
read *,n
allocate(x(n,2))
x=1.d0
y=accumulate(x,3)
print *,shape(y),sum(y)
end
''', '3 2 42.0\n', data='3\n')

    def test_mutually_recursive_integer_results(self):
        self.check('''module m
implicit none
contains
recursive function first(x,n) result(y)
integer,intent(in)::x(:),n
integer::y(size(x))
if(n==0)then
   y=x
else
   y=second(x,n-1)+1
end if
end function
recursive function second(x,n) result(y)
integer,intent(in)::x(:),n
integer::y(size(x))
if(n==0)then
   y=x
else
   y=first(x,n-1)+2
end if
end function
end module
program main
use m
implicit none
print *,first([1,2],3)
end
''', '5 6\n')

    def test_logical_results_and_zero_size(self):
        self.check('''module m
implicit none
contains
recursive pure function invert(x,n) result(y)
logical,intent(in)::x(:)
integer,intent(in)::n
logical::y(size(x))
if(n==0)then
   y=x
else
   y=.not.invert(x,n-1)
end if
end function
end module
program main
use m
implicit none
logical,allocatable::x(:)
integer::i,n
read *,n
allocate(x(n))
x=.true.
do i=1,3
   print *,size(invert(x,i)),count(invert(x,i))
end do
print *,count(invert([.true.,.false.],3))
end
''', '0 0\n0 0\n0 0\n1\n', data='0\n')

    def test_scalar_bound_actual_evaluated_once(self):
        self.check('''module m
implicit none
integer::calls=0
contains
integer function extent()
calls=calls+1
extent=3
end function
recursive function copy(x,n,depth) result(y)
integer,intent(in)::x(:),n,depth
integer::y(n)
if(depth>0)then
   y=copy(x,n,depth-1)
else
   y=x
end if
end function
end module
program main
use m
implicit none
integer::y(3)
y=copy([1,2,3],extent(),2)
print *,y,calls
end
''', '1 2 3 1\n')

    def test_array_actual_evaluated_once(self):
        self.check('''module m
implicit none
integer::calls=0
contains
function values() result(y)
integer::y(3)
calls=calls+1
y=[1,2,3]
end function
recursive function copy(x,n) result(y)
integer,intent(in)::x(:),n
integer::y(size(x))
if(n>0)then
   y=copy(x,n-1)
else
   y=x
end if
end function
end module
program main
use m
implicit none
integer::y(3)
y=copy(values(),2)
print *,y,calls
end
''', '1 2 3 1\n')

    def test_calls_in_repeated_loop_condition(self):
        self.check('''module m
implicit none
contains
recursive function values(n) result(y)
integer,intent(in)::n
integer::y(1)
if(n>0)then
   y=values(n-1)+1
else
   y=0
end if
end function
end module
program main
use m
implicit none
integer::i
i=0
do while(any(values(3-i)>0))
   i=i+1
end do
print *,i
end
''', '3\n')


if __name__ == '__main__':
    unittest.main()
