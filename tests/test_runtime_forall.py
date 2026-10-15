"""Runtime FORALL controls, snapshots and errors at O0/O2 and against gfortran."""
from pathlib import Path
import unittest
import test_deallocate

class RuntimeForall(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_diagonal_and_overlap(self):
        self.check((Path(__file__).parent/'193_runtime_forall.f90').read_text(),'4 3 2 1 99\n1 4\n')

    def test_steps_and_empty(self):
        self.check('''integer::a(5),b(5),i,lo,hi,step
a=[1,2,3,4,5]
b=0
lo=5
hi=1
step=-2
forall(i=lo:hi:step) b(i)=a(i)
print *,b
lo=2
hi=1
step=3
forall(i=lo:hi:step) b(i)=99
print *,b
end
''','1 0 3 0 5\n1 0 3 0 5\n')

    def test_multi_index_sections(self):
        self.check('''integer::a(2,3),i,j,n,m
a=reshape([1,2,3,4,5,6],[2,3])
n=2
m=3
forall(i=1:n,j=1:m) a(i,j)=a(n+1-i,m+1-j)
print *,a
forall(j=1:m) a(:,j)=a(:,m+1-j)
print *,a
end
''','6 5 4 3 2 1\n2 1 4 3 6 5\n')

    def test_selectors_read_destination(self):
        self.check('''integer::a(4),i,n
a=[2,1,4,3]
n=4
forall(i=1:n) a(a(i))=i+10
print *,a
end
''','12 11 14 13\n',reference=False) # gfortran -fcheck=all warns about its array temporary.

    def test_character_and_logical(self):
        self.check('''character(len=2)::a(3)
logical::b(3)
integer::i,n
a=['aa','bb','cc']
b=[.true.,.false.,.false.]
n=3
forall(i=1:n) a(i)=a(n+1-i)
forall(i=1:n) b(i)=b(n+1-i)
print *,a
print *,b
end
''','ccbbaa\nF F T\n')

    def test_zero_iterations_do_not_read(self):
        self.check('''integer::a(2),b(2),i,n
n=0
if(n>0) b=1
forall(i=1:n) a(i)=b(i)
print *,42
end
''','42\n')

    def test_zero_stride(self):
        self.check('''integer::a(2),i,n,s
n=2
s=0
forall(i=1:n:s) a(i)=i
end
''',error='FORALL stride must not be zero',reference=False)

    def test_nondefault_bounds_and_index_kind(self):
        self.check('''integer::a(-2:2),i,n
a=[1,2,3,4,5]
n=2
forall(i=-n:n) a(i)=a(-i)
print *,a
forall(i=-n:n) a(i)=kind(i)-kind(1)
print *,a
end
''','5 4 3 2 1\n0 0 0 0 0\n')

    def test_pure_array_function_rhs(self):
        self.check('''program main
integer::a(2,2),j,n
a=reshape([1,2,3,4],[2,2])
n=2
forall(j=1:n) a(:,j)=copy(a(:,n+1-j))
print *,a
contains
pure function copy(x) result(y)
integer,intent(in)::x(:)
integer::y(size(x))
y=x
end function copy
end program main
''','3 4 1 2\n')

    def test_overlapping_sections_are_rejected(self):
        self.check('''integer::a(3),i,n
n=2
forall(i=1:n) a(i:i+1)=i
end
''',error='same destination element',reference=False)

    def test_input_bound_is_captured_before_writes(self):
        self.check('''integer::a(4),i,n
a=[4,3,2,1]
read(*,*) n
forall(i=1:a(1):n) a(i)=9
print *,a
end
''','9 9 9 9\n',data='1\n')

    def test_duplicate_destinations(self):
        self.check('''integer::a(2),i,n
n=2
forall(i=1:n) a(1)=i
end
''',error='same destination element',reference=False)

    def test_pure_function_bound_and_default_integer_actual(self):
        self.check('''program main
integer::a(3),i,n
n=3
forall(i=1:limit(n)) a(i)=inc(i)
print *,a
contains
pure integer function limit(x)
integer,intent(in)::x
limit=x
end function limit
pure integer function inc(x)
integer,intent(in)::x
inc=x+1
end function inc
end program main
''','2 3 4\n')

    def test_noninteger_control(self):
        self.reject('''integer::a(2),i
real::n
n=2
forall(i=1:n) a(i)=i
end
''','scalar INTEGER')

    def test_integer_eight_endpoint_does_not_overflow(self):
        self.check('''integer,parameter::ik=selected_int_kind(18)
integer(kind=ik)::i,lo,hi,step
integer::a(1)
hi=huge(0_ik)
lo=hi-1
step=2
forall(i=lo:hi:step) a(1)=1
print *,a
hi=-huge(0_ik)
lo=hi+1
step=-2
forall(i=lo:hi:step) a(1)=2
print *,a
end
''','1\n2\n')

    def test_matrix_element_controls(self):
        self.check('''integer::controls(2,2),a(3),i
controls=reshape([1,3,1,0],[2,2])
forall(i=controls(1,1):controls(2,1):controls(1,2)) a(i)=i
print *,a
end
''','1 2 3\n')

    def test_heap_destinations_of_several_ranks(self):
        self.check('''integer,allocatable::a(:),b(:,:),c(:,:,:)
integer::n,i,j,k
n=2
allocate(a(n),b(n,n),c(n,n,n))
a=[1,2]
b=reshape([1,2,3,4],[2,2])
c=reshape([1,2,3,4,5,6,7,8],[2,2,2])
forall(i=1:n) a(i)=a(n+1-i)
forall(i=1:n,j=1:n) b(i,j)=b(n+1-i,n+1-j)
forall(i=1:n,j=1:n,k=1:n) c(i,j,k)=c(n+1-i,n+1-j,n+1-k)
print *,a
print *,b
print *,c
end
''','2 1\n4 3 2 1\n8 7 6 5 4 3 2 1\n')

    def test_heap_real_and_logical_empty_and_repeated(self):
        self.check('''real,allocatable::a(:)
logical,allocatable::b(:)
integer::n,i
do n=2,0,-1
  call setup(n,a,b)
  a=1
  b=.false.
  forall(i=1:n) a(i)=a(n+1-i)+i
  forall(i=1:n) b(i)=.not.b(n+1-i)
  print *,size(a),sum(a),count(b)
  deallocate(a)
  deallocate(b)
end do
contains
subroutine setup(n,a,b)
integer,intent(in)::n
real,allocatable,intent(out)::a(:)
logical,allocatable,intent(out)::b(:)
allocate(a(n))
allocate(b(n))
end subroutine setup
end
''','2 5 2\n1 2 1\n0 0 0\n')

    def test_heap_rank_fifteen(self):
        self.check('''integer,allocatable::a(:,:,:,:,:,:,:,:,:,:,:,:,:,:,:)
integer::i,n
n=2
allocate(a(n,1,1,1,1,1,1,1,1,1,1,1,1,1,1))
a=0
forall(i=1:n) a(i,1,1,1,1,1,1,1,1,1,1,1,1,1,1)=i
print *,a
end
''','1 2\n')

    def test_heap_nondefault_bounds(self):
        self.check('''integer,allocatable::a(:)
integer::i,n
n=2
allocate(a(-1:n-2))
a=[1,2]
forall(i=-1:n-2) a(i)=a(n-3-i)
print *,a
end
''','2 1\n')

if __name__=='__main__':unittest.main()
