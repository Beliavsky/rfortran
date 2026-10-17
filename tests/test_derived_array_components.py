"""Array, ALLOCATABLE and POINTER components of scalar derived types.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_derived_array_components.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class DerivedArrayComponents(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixed_array_component_elements_sections_and_copies(self):
        self.check("""program main
implicit none
type :: vec
integer :: n
real :: x(3)
integer :: m(2,2)
end type vec
type(vec) :: a, b
integer :: i
a%n = 3
do i = 1, 3
a%x(i) = i*1.5
end do
a%m = reshape([1, 2, 3, 4], [2, 2])
b = a
b%x = b%x*2
b%x(2:3) = -1.0
print *, a%n, a%x, sum(b%x), b%x(1), b%m(2,1), sum(b%m)
end program
""", "3 1.5 3.0 4.5 1.0 3.0 2 10\n")

    def test_allocatable_component_deep_copy_and_status(self):
        self.check("""program main
implicit none
type :: bag
integer :: tag
integer, allocatable :: v(:)
end type bag
type(bag) :: x, y, z
allocate(x%v(2:4))
x%v = [1, 2, 3]
x%tag = 5
y = x
x%v = 99
print *, allocated(y%v), lbound(y%v), size(y%v), y%v, y%tag
allocate(z%v(1))
deallocate(z%v)
z%tag = 0
y = z
print *, allocated(y%v)
deallocate(x%v)
print *, allocated(x%v)
end program
""", "T 2 3 1 2 3 5\nF\nF\n")

    def test_constructors_procedures_and_results(self):
        self.check("""module shapes
implicit none
type :: poly
integer :: n = 0
real :: x(4) = 0.0
real, allocatable :: w(:)
end type poly
contains
subroutine scale(p, f)
type(poly), intent(inout) :: p
real, intent(in) :: f
p%x = p%x*f
if (allocated(p%w)) p%w = p%w*f
end subroutine scale
subroutine total(p, s)
type(poly), intent(in) :: p
real, intent(out) :: s
s = sum(p%x(1:p%n))
if (allocated(p%w)) s = s + sum(p%w)
end subroutine total
function make(n) result(p)
integer, intent(in) :: n
type(poly) :: p
integer :: i
p%n = n
do i = 1, n
p%x(i) = real(i)
end do
allocate(p%w(n))
p%w = 10.0
end function make
end module shapes
program main
use shapes
implicit none
type(poly) :: a, b, c
real :: s1, s2
real :: v(4) = [1.0, 2.0, 3.0, 4.0]
a = poly(2, v, null())
print *, a%n, a%x, allocated(a%w)
b = poly(3, [5.0, 6.0, 7.0, 8.0], [1.0, 2.0])
call scale(b, 2.0)
print *, b%x, b%w
call total(a, s1)
call total(b, s2)
print *, s1, s2
c = make(3)
print *, c%n, c%x, c%w
call total(poly(1, v, [100.0]), s1)
print *, s1
end program
""", "2 1.0 2.0 3.0 4.0 F\n"
     "10.0 12.0 14.0 16.0 2.0 4.0\n"
     "3.0 42.0\n"
     "3 1.0 2.0 3.0 0.0 10.0 10.0 10.0\n"
     "101.0\n")

    def test_pointer_components_associate_copy_and_nullify(self):
        self.check("""program main
implicit none
type :: view
real, pointer :: p(:) => null()
integer, pointer :: k => null()
end type view
type(view) :: a, b
real, target :: t(4) = [1.0, 2.0, 3.0, 4.0]
integer, target :: j = 7
print *, associated(a%p), associated(a%k)
a%p => t(2:3)
a%k => j
b = a
b%p(1) = 20.0
print *, associated(b%p), sum(b%p), b%k, t
b = view(null(), null())
print *, associated(b%p), associated(b%k)
end program
""", "F F\nT 23.0 7 1.0 20.0 3.0 4.0\nF F\n")

    def test_unsupported_component_forms_are_diagnosed(self):
        for declaration, message in [
            ("type(other) :: inner", "derived-type components of derived type"),
            ("character(len=:), allocatable :: s", "ALLOCATABLE scalar component"),
        ]:
            self.reject(f"""program main
implicit none
type :: other
integer :: i
end type other
type :: t
{declaration}
end type t
type(t) :: x
print *, 1
end program
""", message)

    def test_allocatable_component_from_expressions(self):
        self.check("""program main
implicit none
type :: t
real, allocatable :: v(:)
end type t
type(t) :: x
real :: a(3) = 2.0
x = t(a*3)
print *, size(x%v), x%v
x = t(x%v(1:2) + 1)
print *, size(x%v), x%v
end program
""", "3 6.0 6.0 6.0\n2 7.0 7.0\n")


if __name__ == "__main__":
    unittest.main()
