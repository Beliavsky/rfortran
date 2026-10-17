"""Data/procedure name shadowing must be independent of unrelated derived types."""
import unittest
import test_deallocate
from test_record_read import ROOT

TYPE = "type :: unrelated_type\ninteger :: value\nend type unrelated_type\n"

class DerivedShadowing(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def compare(self, source, expected):
        for derived in (False, True):
            with self.subTest(derived=derived):
                self.check(source.replace("!TYPE", TYPE if derived else ""), expected)

    def test_reproducer(self):
        source = (ROOT / "tests/programs/xivec_shadow.f90").read_text(encoding="utf-8")
        self.compare(source.replace("type :: unrelated_type\n   integer :: value\nend type unrelated_type", "!TYPE"), "7\n")

    def test_array_dummy_rank_three(self):
        self.compare("""program main
implicit none
!TYPE
integer :: values(1,1,2)
values=9
call show(values)
contains
integer function ivec(n) result(j)
integer,intent(in)::n
j=n
end function
subroutine show(ivec)
integer,intent(in)::ivec(:,:,:)
print *,ivec(1,1,2)
end subroutine
end program
""", "9\n")

    def test_block_array_shadows_function(self):
        self.compare("""program main
implicit none
!TYPE
block
integer :: ivec(1,2)
ivec=5
print *,ivec(1,2)
end block
contains
integer function ivec(n) result(j)
integer,intent(in)::n
j=n+1
end function
end program
""", "5\n")

    def test_nested_internal_array_dummy(self):
        self.check("""module shadowing_mod
implicit none
type :: unrelated_type
integer :: value
end type unrelated_type
contains
integer function ivec(n) result(j)
integer,intent(in)::n
j=n
end function
subroutine outer(values)
integer,intent(in)::values(:,:)
call show(values)
contains
subroutine show(ivec)
integer,intent(in)::ivec(:,:)
print *,ivec(1,2)
end subroutine
end subroutine
end module
program main
use shadowing_mod
implicit none
integer :: values(1,2)
values=7
call outer(values)
end program
""", "7\n")

    def test_derived_function_shadowed_by_array(self):
        self.check("""program main
implicit none
type :: box
integer :: value
end type box
type(box)::b
integer :: values(1,2)
values=8
call show(values)
b=ivec(4)
print *,b%value
contains
function ivec(n) result(b)
integer,intent(in)::n
type(box)::b
b=box(n)
end function
subroutine show(ivec)
integer,intent(in)::ivec(:,:)
print *,ivec(1,2)
end subroutine
end program
""", "8\n4\n")

    def test_array_shadows_derived_argument_procedure(self):
        self.check("""program main
implicit none
type :: box
integer :: value
end type box
type(box)::b
integer :: values(1,2)
values=6
call show(values)
b=box(3)
print *,ivec(b)
contains
integer function ivec(b) result(j)
type(box),intent(in)::b
j=b%value
end function
subroutine show(ivec)
integer,intent(in)::ivec(:,:)
print *,ivec(1,2)
end subroutine
end program
""", "6\n3\n")

if __name__ == "__main__": unittest.main()
