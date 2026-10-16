"""Constructor, generic, descriptor and external READ utility regressions."""
import unittest
import test_deallocate
from test_record_read import ROOT


class RemainingUtil(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/205_util_families.f90').read_text(),'utility driver regressions passed\n')

    def test_preconnected_integer_unit_and_wide_input(self):
        self.check('''integer(kind=8)::n
integer::ios
read(5,*,iostat=ios)n
print *,ios,n
end
''','0 -9223372036854775808\n',data='-9223372036854775808\n')

    def test_long_integer_record_is_not_truncated(self):
        self.check('''integer::u,n,ios
character(4102)::text
open(newunit=u,status='scratch')
text=''
text(4101:4102)='42'
write(u,'(a)')text
write(u,'(a)')'17'
rewind(u)
read(u,*,iostat=ios)n
print *,ios,n
read(u,*)n
print *,n
close(u)
end
''','0 42\n17\n')

    def test_character_runtime_constructors(self):
        self.check('''character(2)::words(2),a(4),b(4),empty(0)
integer::n,i,j
words=['ab','cd']
i=99
j=88
read(*,*)n
a=[(words,i=1,n)]
b=[((words(j),i=1,n),j=1,size(words))]
print *,a,b,i,j
empty=[(words,i=1,0)]
print *,size(empty)
end
''','abcdabcdababcdcd 99 88\n0\n',data='2\n')

    def test_elemental_generic_array_keywords(self):
        self.check('''module m
interface f
module procedure scalar_f
end interface
contains
elemental integer function scalar_f(x,offset) result(y)
integer,intent(in)::x
integer,intent(in),optional::offset
y=x
if(present(offset))y=y+offset
end function
end module
program main
use m
integer::a(2,2),empty(0),out(0)
a=reshape([1,2,3,4],[2,2])
print *,f(x=a,offset=2)
out=f(empty)
print *,sum(f(a)),size(out)
end
''','3 4 5 6\n10 0\n')

    def test_matrix_allocatable_output(self):
        self.check('''integer,allocatable::a(:,:)
call copy(reshape([1,2,3,4],[2,2]),a)
print *,shape(a),a
call copy(reshape([5,6,7],[1,3]),a)
print *,shape(a),a
deallocate(a)
contains
subroutine copy(x,y)
integer,intent(in)::x(:,:)
integer,allocatable,intent(out)::y(:,:)
allocate(y(size(x,1),size(x,2)))
y=x
end subroutine
end
''','2 2 1 2 3 4\n1 3 5 6 7\n')

    def test_external_integer_read_status_and_blank_records(self):
        self.check('''integer::u,n,ios
open(newunit=u,status='scratch',action='readwrite')
write(u,'(a)')''
write(u,'(a)')'   '
write(u,'(a)')'-42'
write(u,'(a)')'bad'
write(u,'(a)')'17'
rewind(u)
read(u,*,iostat=ios)n
print *,ios,n
read(u,*,iostat=ios)n
print *,ios>0
read(u,*)n
print *,n
read(u,*,iostat=ios)n
print *,ios<0
close(u)
end
''','0 -42\nT\n17\nT\n')

    def test_external_read_rejected_in_pure(self):
        self.reject('''pure subroutine bad(u,n)
integer,intent(in)::u
integer,intent(out)::n
read(u,*)n
end subroutine
end
''','PURE procedure cannot perform external I/O')

    def test_pure_internal_array_element_read_stays_valid(self):
        self.check('''character(3)::lines(1)
integer::n
lines=[' 42']
call parse(lines,n)
print *,n
contains
pure subroutine parse(lines,n)
character(*),intent(in)::lines(:)
integer,intent(out)::n
read(lines(1),*)n
end subroutine
end
''','42\n')

    def test_character_matrix_reshape_order_and_padding(self):
        self.check('''character(2),allocatable::a(:,:)
call copy(reshape(['ab','cd'],[2,2],pad=['ef'],order=[2,1]),a)
print *,shape(a),a
call copy(reshape([character(2)::],[0,2]),a)
print *,shape(a),size(a)
contains
subroutine copy(x,y)
character(*),intent(in)::x(:,:)
character(*),allocatable,intent(out)::y(:,:)
allocate(y(size(x,1),size(x,2)))
y=x
end subroutine
end
''','2 2 abefcdef\n0 2 0\n')

    def test_allocatable_outputs_higher_ranks(self):
        for rank in (4,15):
            with self.subTest(rank=rank):
                dims=','.join([':']*rank)
                shape=','.join(['1']*(rank-1)+['2'])
                allocate=', &\n  &'.join(f'size(x,{i})' for i in range(1,rank+1))
                self.check(f'''integer,allocatable::a({dims})
call copy(reshape([7,9],[{shape}]),a)
print *,size(a),sum(a)
contains
subroutine copy(x,y)
integer,intent(in)::x({dims})
integer,allocatable,intent(out)::y({dims})
allocate(y({allocate}))
y=x
end subroutine
end
''','2 16\n')

    def test_generic_prefers_matching_rank(self):
        self.check('''module m
interface f
module procedure scalar_f,array_f
end interface
contains
elemental integer function scalar_f(x) result(y)
integer,intent(in)::x
y=x+1
end function
integer function array_f(x) result(y)
integer,intent(in)::x(:)
y=sum(x)+10
end function
end module
program main
use m
print *,f(2),f([2,3])
end
''','3 15\n')


if __name__=='__main__':unittest.main(verbosity=2)
