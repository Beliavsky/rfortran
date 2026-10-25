"""Preserve allocatable descriptors through nested type-bound calls."""
import unittest
from pathlib import Path
import test_deallocate as helpers

FIXTURE = (Path(__file__).parent / "programs/xallocatable_component_forward.f90").read_text()

class AllocatableComponentForward(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_real_matrix(self):
        self.check(FIXTURE, "2 3 3\n")

    def test_integer_matrix_with_lower_bounds(self):
        source = FIXTURE.replace("real, allocatable", "integer, allocatable")
        source = source.replace(" > 0.0", " > 0")
        source = source.replace("x%values(2,3)", "x%values(-1:0,4:6)")
        source = source.replace("[1.,-1.,2.,-2.,3.,-3.]", "[1,-1,2,-2,3,-3]")
        self.check(source, "2 3 3\n")

    def test_rank_three(self):
        source = FIXTURE.replace("(:,:)", "(:,:,:)")
        source = source.replace("x%values(2,3)", "x%values(2,1,3)")
        source = source.replace("[2,3]", "[2,1,3]")
        self.check(source, "2 1 3 3\n")

    def test_logical_component(self):
        source = FIXTURE.replace("real, allocatable", "logical, allocatable")
        source = source.replace("self%values > 0.0", "self%values")
        source = source.replace("[1.,-1.,2.,-2.,3.,-3.]", "[.true.,.false.,.true.,.false.,.true.,.false.]")
        self.check(source, "2 3 3\n")

    def test_nested_write_updates_original_storage(self):
        source = FIXTURE.replace("intent(in) :: self", "intent(inout) :: self")
        source = source.replace("mask = self%values > 0.0",
                                "self%values = self%values + 10.0\nmask = self%values > 0.0")
        source = source.replace("call x%inspect()", "call x%inspect()\nprint *, sum(x%values)")
        self.check(source, "2 3 6\n60.0\n")

    def test_unallocated_component(self):
        source = FIXTURE.replace('if (.not. allocated(self%values)) error stop "values not allocated"',
                                 'print *, allocated(self%values)\nallocate(mask(0,0))\nreturn')
        source = source.replace("allocate(x%values(2,3))", "")
        source = source.replace("x%values = reshape([1.,-1.,2.,-2.,3.,-3.],[2,3])", "")
        self.check(source, "F\n0 0 0\n")

    def test_expression_is_not_allocatable(self):
        self.reject("""implicit none
integer, allocatable :: a(:)
allocate(a(3))
a=1
call take(a + 1)
contains
subroutine take(x)
integer, allocatable, intent(in) :: x(:)
print *, size(x)
end subroutine
end
""", "requires an ALLOCATABLE variable")

if __name__ == "__main__":
    unittest.main()
