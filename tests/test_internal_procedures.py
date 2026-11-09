"""Internal procedures and borrowed host state in interpreter/native O0/O2."""
import unittest
import test_deallocate
import subprocess
import tempfile
from pathlib import Path
from test_record_read import COMPILER


class InternalProcedures(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_recursive_array_helpers(self):
        self.check('''module sorting
contains
subroutine sort(a,order)
integer,intent(inout)::a(:)
integer,intent(out)::order(:)
integer::i
do i=1,size(a)
order(i)=i
end do
call split(1,size(a))
contains
recursive subroutine split(lo,hi)
integer,intent(in)::lo,hi
integer::i,j,pivot,tmp
if(lo>=hi) return
pivot=a((lo+hi)/2)
i=lo;j=hi
do
do while(a(i)<pivot)
i=i+1
end do
do while(a(j)>pivot)
j=j-1
end do
if(i>j) exit
tmp=a(i);a(i)=a(j);a(j)=tmp
tmp=order(i);order(i)=order(j);order(j)=tmp
i=i+1;j=j-1
if(i>j) exit
end do
if(lo<j) call split(lo,j)
if(i<hi) call split(i,hi)
end subroutine
end subroutine
end module
program main
use sorting
integer::a(6),order(6)
a=[6,1,4,2,5,3]
call sort(a,order)
print *,a
print *,order
a=[3,3,1,1,2,2]
call sort(a,order)
print *,a
end
''','1 2 3 4 5 6\n2 4 6 3 5 1\n1 1 2 2 3 3\n')

    def test_recursive_function_host_read(self):
        self.check('''program main
integer::outer
print *,outer(4),outer(2)
end
integer function outer(n) result(answer)
integer,intent(in)::n
integer::factor
factor=3
answer=power(n)
contains
recursive integer function power(k) result(value)
integer,intent(in)::k
if(k==0) then
value=1
else
value=factor*power(k-1)
end if
end function
end function
''','81 9\n')

    def test_host_writes_siblings_and_shadowing(self):
        self.check('''call work()
call work()
end
subroutine work()
integer::x
x=20
call first()
print *,x
contains
subroutine first()
integer::x
x=100
call second()
print *,x
end subroutine
subroutine second()
x=x+1
end subroutine
end subroutine
''','100\n21\n100\n21\n')

    def test_multidimensional_host_and_bounds(self):
        self.check('''call work()
end
subroutine work()
real(8)::a(-1:0,2:3,4:5)
a=2.0d0
call change()
print *,a(-1,3,5),sum(a)
contains
subroutine change()
print *,size(a),size(a,2),lbound(a),ubound(a)
a(-1,3,5)=9.0d0
end subroutine
end subroutine
''','8 2 -1 2 4 0 3 5\n9 23\n')

    def test_host_loop_variable_and_while_condition(self):
        self.check('''call work()
end
subroutine work()
integer::i,n
n=0
call run()
print *,i,n
contains
subroutine run()
do i=1,3
n=n+i
end do
do while(n<8)
n=n+1
cycle
end do
end subroutine
end subroutine
''','4 8\n')

    def test_uninitialized_host_and_readonly_rejection(self):
        self.check('''call work()
end
subroutine work()
integer::x
call read_x()
contains
subroutine read_x()
print *,x
end subroutine
end subroutine
''',error='uninitialized variable `x`')
        self.reject('''subroutine work(x)
integer,intent(in)::x
call change()
contains
subroutine change()
x=2
end subroutine
end subroutine
''','cannot define host-associated INTENT(IN)')

    def test_output_host_and_implicit_function_result(self):
        self.check('''integer::x
call work(x)
print *,x
end
subroutine work(answer)
integer,intent(out)::answer
integer::base
base=10
call set_answer(k=3)
contains
subroutine set_answer(k)
integer,intent(in)::k
answer=value(k)
end subroutine
integer function value(k)
integer,intent(in)::k
value=base+k
end function
end subroutine
''','13\n')

    def test_same_helper_name_in_distinct_hosts(self):
        self.check('''call first()
call second()
end
subroutine first()
integer::x
x=1
call helper()
print *,x
contains
subroutine helper()
x=x+10
end subroutine
end subroutine
subroutine second()
integer::x
x=2
call helper()
print *,x
contains
subroutine helper()
x=x+20
end subroutine
end subroutine
''','11\n22\n')

    def test_rank_fifteen_host_array(self):
        dims=','.join(['2']+['1']*14)
        index=','.join(['2']+['1']*14)
        self.check(f'''call work()
end
subroutine work()
integer::a({dims})
a=4
call change()
print *,a({index}),sum(a)
contains
subroutine change()
a({index})=9
end subroutine
end subroutine
''','9 13\n')

    def test_host_input_dummy(self):
        self.check('''program main
integer::outer
print *,outer(3),outer(7)
end
integer function outer(n) result(answer)
integer,intent(in)::n
answer=twice()
contains
integer function twice()
twice=2*n
end function
end function
''','6 14\n')

    def test_function_result_declared_in_body(self):
        self.check('''call work()
end
subroutine work()
real::factor
factor=1.25
print *,value()
contains
function value()
real::value
value=factor+2.5
end function
end subroutine
''','3.75\n')

    def test_separate_objects_keep_helpers_private(self):
        with tempfile.TemporaryDirectory(prefix='internal-objects-') as tmp:
            directory=Path(tmp)
            objects=[]
            for i in (1,2):
                source=directory/f'work{i}.f90'
                obj=directory/f'work{i}.obj'
                source.write_text(f'''subroutine work{i}()
integer::x
x={i}
call helper()
print *,x
contains
subroutine helper()
x=x+{i*10}
end subroutine
end subroutine
''')
                result=subprocess.run([COMPILER,'emit-obj',source,obj],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                objects.append(obj)
            main=directory/'main.f90';exe=directory/'main.exe'
            main.write_text('call work1()\ncall work2()\nend\n')
            result=subprocess.run([COMPILER,*objects,main,'-o',exe],capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([exe],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['11','22'])

    def test_host_definitions_by_read_and_procedure_arguments(self):
        # READ, procedure arguments and intrinsic subroutines define host
        # variables, which live in module storage.
        self.check('''call work()
end
subroutine work()
integer::x
real::r
call helper()
print *,x,r>=0.0,r<1.0
contains
subroutine helper()
read(*,*) x
call setter(x)
x=x+1
call random_number(r)
end subroutine
subroutine setter(y)
integer,intent(inout)::y
y=2*y
end subroutine
end subroutine
''',' 11 T T\n',data='5\n')

    def test_unsupported_host_definition_paths_are_rejected(self):
        self.reject('''subroutine work()
integer::x
call helper()
contains
subroutine helper()
print *,size(x)
end subroutine
end subroutine
''','SIZE requires a rank-one array')
        self.reject('''subroutine work()
contains
function value()
value=2
end function
end subroutine
''','explicitly declared result type')
        # An implicitly typed host variable is host associated.
        self.check('''program main
x=1
call helper()
print *,x
contains
subroutine helper()
x=x+1.5
end subroutine
end program
''','2.5\n')

    def test_main_program_host(self):
        self.check('''program main
implicit none
integer :: i
i=-1
call bump()
print *,i
contains
subroutine bump()
i=i+1
print *,i
end subroutine
end program
''','0\n0\n')
        self.check('''program h7
implicit none
integer :: n, a(5), i
real :: total
double precision :: d
logical :: done
n = 5
a = [(i*i, i = 1, 5)]
total = 0
d = 1.5d0
done = .false.
call accumulate()
print *, total, count_big(10), done
call scale(2)
print *, a, d
contains
subroutine accumulate()
integer :: i
do i = 1, n
   total = total + a(i)
end do
done = .true.
end subroutine
integer function count_big(limit)
integer, intent(in) :: limit
integer :: j
count_big = 0
do j = 1, n
   if (a(j) > limit) count_big = count_big + 1
end do
end function
subroutine scale(k)
integer, intent(in) :: k
integer :: j
do j = 1, n
   a(j) = a(j) * k
end do
d = d * k
call accumulate()
end subroutine
end program
''','55.0 2 T\n2 8 18 32 50 3.0\n')


    def test_host_constant_arrays(self):
        # A named constant array of the host, subscripted at run time in an
        # internal function with an array argument and in a subroutine,
        # with a shape given by another constant.
        self.check('''program main
implicit none
integer, parameter :: n = 3
integer, parameter :: idx(n) = [3,1,2], twice(n) = [6,2,4]
real :: x(n) = [10.,20.,30.]
print *, pick(x), idx(2)
call show()
contains
real function pick(y)
real, intent(in) :: y(n)
integer :: k
pick = 0
do k = 1, n
  pick = pick + twice(k)*y(idx(k))
end do
end function
subroutine show()
integer :: k
do k = 1, n
  print *, idx(k), twice(k)
end do
end subroutine
end program
''', '280.0 1\n3 6\n1 2\n2 4\n')
        self.check('''module m
contains
real function outer(y)
real, intent(in) :: y(3)
integer, parameter :: idx(3) = [3,1,2]
outer = inner(y)
contains
real function inner(z)
real, intent(in) :: z(3)
integer :: k
inner = 0
do k = 1, 3
  inner = inner + k*z(idx(k))
end do
end function
end function
end module
program p
use m
print *, outer([10.,20.,30.])
end
''', '110.0\n')

    def test_whole_arrays_sections_allocatables_and_characters_of_a_main_program(self):
        # The host variables live in module storage, with all its support.
        self.check('''program main
implicit none
real :: a(5)
real, allocatable :: b(:)
character(len=10) :: name
integer :: k, n
a = [(real(k), k=1,5)]
call scale(2.0)
call part
print '(5f5.1)', a
print '(f6.1)', total()
call grow(3)
print '(3f4.1,i3)', b, size(b)
name = 'abc'
call greet
call add(n)
print '(i0)', n
call twice
print '(5f5.1)', a
contains
subroutine scale(f)
real, intent(in) :: f
a = a * f
end subroutine
subroutine part
a(2:3) = -1.
end subroutine
real function total()
total = sum(a)
end function
subroutine grow(m)
integer, intent(in) :: m
allocate(b(m))
b = 1.5
end subroutine
subroutine greet
print '(2a)', 'hi ', trim(name)
name = 'xyz'
end subroutine
subroutine add(m)
integer, intent(out) :: m
m = count(a > 0.) + len_trim(name)
end subroutine
subroutine twice
call dbl(a)
end subroutine
subroutine dbl(x)
real, intent(inout) :: x(:)
x = 2*x
end subroutine
end program main
''', '''  2.0 -1.0 -1.0  8.0 10.0
  18.0
 1.5 1.5 1.5  3
hi abc
6
  4.0 -2.0 -2.0 16.0 20.0
''')

    def test_procedure_hosts(self):
        # A host that cannot call itself has one instance of its variables;
        # its ALLOCATABLE variables are deallocated when it returns, a local
        # of an internal procedure hides a host variable, and a recursive
        # host keeps its own variables in each call.
        self.check('''module ops
implicit none
integer, parameter :: n = 4
contains
subroutine stats(x, mean, label)
real, intent(in) :: x(:)
real, intent(out) :: mean
character(len=*), intent(out) :: label
real, allocatable :: work(:)
character(len=12) :: tag
integer :: calls = 0
allocate(work(size(x)))
work = x
call centre
mean = sum(work) / size(work) + total()
call stamp
label = tag
contains
subroutine centre
work = work - minval(work)
end subroutine
real function total()
real :: work
work = 0.
total = work
end function
subroutine stamp
calls = calls + 1
write(tag, '(a,i0)') 'calls=', calls
end subroutine
end subroutine
integer function fact(k) result(r)
integer, intent(in) :: k
integer :: acc(n)
acc = 1
call fill
r = product(acc(1:k))
contains
subroutine fill
integer :: i
do i = 2, n
acc(i) = i
end do
end subroutine
end function
end module ops
recursive integer function tri(k) result(r)
integer, intent(in) :: k
integer :: here
here = k
r = 0
call add
contains
recursive subroutine add
if (here > 0) r = here + tri(here - 1)
end subroutine
end function
program main
use ops
implicit none
interface
recursive integer function tri(k) result(r)
integer, intent(in) :: k
end function
end interface
real :: m
character(len=12) :: lab
call stats([3., 5., 7.], m, lab)
print '(f6.2,1x,a)', m, trim(lab)
call stats([1., 2.], m, lab)
print '(f6.2,1x,a)', m, trim(lab)
print '(i0,1x,i0)', fact(3), fact(4)
print '(i0)', tri(4)
end program main
''', '''  2.00 calls=1
  0.50 calls=2
6 24
10
''')

    def test_private_components_of_use_associated_objects(self):
        # A host object of a type with PRIVATE components, and a module
        # object, are checked wherever they are used.
        self.reject('''module boxes
type box
integer, private :: value = 1
end type
type(box) :: shared
end module
program p
use boxes
print *, shared%value
end
''', 'PRIVATE to module `boxes`')


if __name__=='__main__':
    unittest.main(verbosity=2)
