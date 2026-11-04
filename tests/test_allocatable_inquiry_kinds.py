"""Descriptor sizes must not change the public INTEGER kind of inquiries."""
import unittest
import test_deallocate


class AllocatableInquiryKinds(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_merge_size_fixture(self):
        source = (test_deallocate.ROOT / "tests/programs/xmerge_alloc.f90").read_text()
        self.check(source, "2\n")

    def test_merge_still_rejects_different_integer_kinds(self):
        self.reject("""program main
implicit none
real, allocatable :: x(:)
allocate(x(2))
print *, merge(size(x), -1_8, allocated(x))
end program
""", "MERGE sources must have the same type, kind")

    def test_runtime_sizes_and_bounds_have_default_kind(self):
        self.check("""program main
implicit none
real, allocatable :: x(:,:), y(:)
integer :: n
read *, n
allocate(x(-1:n,3:4), y(n))
print *, kind(size(x)), kind(size(x,1)), kind(size(y))
print *, kind(lbound(x,1)), kind(ubound(x,2))
print *, merge(size(x),-1,allocated(x))
print *, merge(size(x,1),-1,allocated(x))
print *, merge(lbound(x,1),0,allocated(x))
print *, merge(ubound(x,2),0,allocated(x))
print *, [size(y), -1]
end program
""", "4 4 4 4 4 10 5 -1 4 3 -1\n", data="3\n")


if __name__ == "__main__":
    unittest.main()
