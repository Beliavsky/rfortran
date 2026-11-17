"""Valid programs that rfort rejected, found by the gfortran test suite.

- An internal function without a RESULT clause whose result is an array,
  `integer :: f(n)`, was treated as scalar: the result went unassigned (or one
  value filled the whole array).
- A variable that was never set, passed to an INTENT(IN) dummy argument, was
  rejected as a read even when the procedure never reads the dummy; passing a
  variable is not a reference to its value.
- A procedure pointer, or procedure pointer component, with an initial target
  in its declaration (`procedure(f), pointer :: p => f`) lost the target.
- Errors about undefined values named compiler temporaries, such as
  `@sub_local_2`, instead of the programmer's variables.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_valid_program_rejections.py
"""
import unittest

import test_deallocate


class ValidProgramRejections(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_array_results_named_like_the_function(self):
        self.check("""program p
implicit none
integer :: a(2), b(3)
a = set_by_call()
b = counted(size(b))
print *, a, b
contains
function set_by_call()
integer :: set_by_call(2)
call fill(set_by_call)
end function set_by_call
subroutine fill(v)
integer :: v(2)
v = [7, 8]
end subroutine fill
pure function counted(n)
integer, intent(in) :: n
integer :: counted(n)
integer :: i
do i = 1, n
   counted(i) = 10*i
end do
end function counted
end program p
""", "7 8 10 20 30\n")

    def test_unread_intent_in_arguments_need_not_be_defined(self):
        self.check("""program p
implicit none
real :: never_set, y
integer :: k(3)
k = 1
y = 2.0
call show(never_set, y, k)
call show_scalar(never_set, 4)
contains
subroutine show(x, y, a)
real, intent(in) :: x, y
integer, intent(in) :: a(3)
print *, y, sum(a)
end subroutine show
subroutine show_scalar(x, n)
real, intent(in) :: x
integer, intent(in) :: n
print *, n
end subroutine show_scalar
end program p
""", "2.0 3\n4\n")

    def test_reading_the_undefined_argument_is_still_an_error(self):
        # The read is reported, naming the dummy argument that is read.
        self.reject("""program p
implicit none
real :: never_set, y
integer :: k(3)
k = 1
y = 2.0
call show(never_set, y, k)
contains
subroutine show(x, y, a)
real, intent(in) :: x, y
integer, intent(in) :: a(3)
print *, x, y, sum(a)
end subroutine show
end program p
""", "read of uninitialized variable `x`")

    def test_messages_name_the_programs_variables(self):
        self.reject("""program p
implicit none
real :: x(3)
call copy(x)
print *, x
contains
subroutine copy(out)
real, intent(inout) :: out(:)
real :: r(size(out))
out = r
end subroutine copy
end program p
""", "read of uninitialized variable `r`")

    def test_initialized_procedure_pointers(self):
        self.check("""module m
implicit none
type :: t
   procedure(f2), pointer, nopass :: c => f2
   procedure(f2), pointer, nopass :: none => null()
end type t
contains
integer function f1()
f1 = 42
end function f1
integer function f2()
f2 = 43
end function f2
end module m
program p
use m
implicit none
procedure(f1), pointer :: pp => f1, qq => null()
type(t) :: u, v
print *, pp(), associated(pp), associated(qq), associated(pp, f1)
pp => f2
print *, pp(), associated(pp, f1)
print *, u%c(), v%c(), associated(u%c), associated(u%none)
u%c => f1
print *, u%c(), v%c()
end program p
""", "42 T F T\n43 F\n43 43 T F\n42 43\n")


if __name__ == "__main__":
    unittest.main()
