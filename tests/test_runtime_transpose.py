"""Runtime TRANSPOSE shape, ordering and argument association."""
import unittest
import test_deallocate as helpers

class RuntimeTranspose(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_assignment_overlap_and_empty(self):
        self.check("""implicit none
integer :: n
integer,allocatable :: a(:,:),b(:,:)
n=2
allocate(a(-1:0,4:6))
a=reshape([1,2,3,4,5,6],[2,3])
b=transpose(a)
print *,shape(b),b
a=transpose(a)
print *,shape(a),a
deallocate(a)
allocate(a(n,0))
b=transpose(a)
print *,shape(b),size(b)
end
""", "3 2 1 3 5 2 4 6\n3 2 1 3 5 2 4 6\n0 2 0\n")

    def test_direct_argument_sections_and_expression(self):
        self.check("""implicit none
integer :: n
real,allocatable :: a(:,:)
n=2
allocate(a(n,3))
a=reshape([1.25,2.5,3.75,4.0,5.5,6.0],[2,3])
call show(transpose(a))
call show(transpose(a(:,3:1:-2)))
call show(transpose(a+1.0))
contains
subroutine show(x)
real,intent(in)::x(:,:)
print *,shape(x),sum(x)
print *,x
end subroutine
end
""", "3 2 23.0\n1.25 3.75 5.5 2.5 4.0 6.0\n2 2 15.25\n5.5 1.25 6.0 2.5\n3 2 29.0\n2.25 4.75 6.5 3.5 5.0 7.0\n")

    def test_assumed_shape_dummy(self):
        self.check("""implicit none
integer::n
real,allocatable::a(:,:)
n=2
allocate(a(n,3))
a=reshape([1.,2.,3.,4.,5.,6.],[2,3])
call work(a)
contains
subroutine work(x)
real,intent(in)::x(:,:)
real,allocatable::y(:,:)
y=transpose(x)
print *,shape(y),y
end subroutine
end
""", "3 2 1 3 5 2 4 6\n")

    def test_logical_nested(self):
        self.check("""implicit none
integer::n
logical,allocatable::a(:,:),b(:,:)
n=2
allocate(a(n,3))
a=reshape([.true.,.false.,.false.,.true.,.true.,.false.],[2,3])
b=transpose(a)
print *,shape(b),b
b=transpose(transpose(a))
print *,shape(b),all(b.eqv.a)
end
""", "3 2 T F T F T F\n2 3 T\n")

if __name__ == '__main__': unittest.main()
