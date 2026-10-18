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

    def test_unsupported_host_definition_paths_are_rejected(self):
        for statement,extra,message in [
            ('read(*,*) x','','READ into host-associated'),
            ('call random_number(x)','','ordinary subroutines'),
            ('call setter(x)','subroutine setter(y)\ninteger,intent(out)::y\ny=2\nend subroutine\n','writable dummy'),
        ]:
            source=f'''subroutine work()
integer::x
call helper()
contains
subroutine helper()
{statement}
end subroutine
{extra}end subroutine
'''
            with self.subTest(statement=statement):self.reject(source,message)
        self.reject('''subroutine work()
integer::x
call helper()
contains
subroutine helper()
print *,size(x)
end subroutine
end subroutine
''','requires a host-associated array')
        self.reject('''subroutine work()
contains
function value()
value=2
end function
end subroutine
''','explicitly declared result type')
        self.reject('''subroutine work()
x=1
call helper()
contains
subroutine helper()
x=2
end subroutine
end subroutine
''','requires an explicit declaration')

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


if __name__=='__main__':
    unittest.main(verbosity=2)
