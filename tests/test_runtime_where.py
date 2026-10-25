"""Runtime WHERE preserves masks and evaluates only selected RHS elements."""
from pathlib import Path
import unittest
import test_deallocate as helpers

class RuntimeWhere(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_type_bound_nan_fill(self):
        self.check((Path(__file__).parent / "programs/xwhere_runtime_fill.f90").read_text(),
                   "T T\nT T\nT\n")

    def test_snapshot_elsewhere_and_lower_bounds(self):
        self.check("""implicit none
integer,allocatable::a(:,:),b(:,:)
integer::n
n=3
allocate(a(-1:0,4:n+3),b(2,n))
a=reshape([1,2,3,4,5,6],[2,n])
b=10
where(a<=3)
 a=a+10
 b=a
elsewhere(a<=5)
 a=-1
elsewhere
 a=-2
end where
print *,a
print *,b
end
""", "11 12 13 -1 -1 -2\n11 12 13 10 10 10\n")

    def test_excluded_uninitialized_rhs_and_nested_mask(self):
        self.check("""implicit none
integer,allocatable::a(:),b(:)
integer::n
n=4
allocate(a(n),b(n))
a=[1,0,1,0]
b(1)=7
b(3)=9
where(a>0)
 where(b>7)
  a=b
 elsewhere
  a=-1
 end where
end where
print *,a
end
""", "-1 0 9 0\n", reference=False)

    def test_empty_and_rank_three(self):
        self.check("""implicit none
real,allocatable::a(:,:,:)
integer::n
do n=0,2
 allocate(a(2,n,2))
 a=1
 where(a>2) a=9
 where(a==1) a=sqrt(a+3)
 print *,size(a),int(sum(a))
 deallocate(a)
end do
end
""", "0 0\n4 8\n8 16\n")

    def test_assumed_shape_dummy_and_elemental_rhs(self):
        self.check("""module m
contains
pure elemental integer function twice(x)
integer,intent(in)::x
twice=x*2
end function
subroutine update(a)
integer,intent(inout)::a(:,:)
where(a>1) a=twice(a)
end subroutine
end module
use m
implicit none
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a=reshape([0,1,2,3,4,5],[2,n])
call update(a)
print *,a
end
""", "0 1 4 6 8 10\n")

    def test_user_elemental_excludes_uninitialized_elements(self):
        self.check("""module m
contains
pure elemental integer function twice(x)
integer,intent(in)::x
twice=x*2
end function
end module
use m
implicit none
integer,allocatable::a(:),b(:)
integer::n
n=3
allocate(a(n),b(n))
a=[1,0,1]
b(1)=7
b(3)=9
where(a>0) a=twice(b)
print *,a
end
""", "14 0 18\n", reference=False)

    def test_runtime_shape_mismatch(self):
        self.check("""implicit none
integer,allocatable::a(:,:),b(:,:)
integer::n
n=3
allocate(a(2,n),b(n,2))
a=1
b=2
where(a>0) b=9
end
""", error="WHERE destination shape does not conform")

if __name__ == "__main__":
    unittest.main()
