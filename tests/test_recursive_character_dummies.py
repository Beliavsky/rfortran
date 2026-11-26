"""Recursive procedures with INTENT(IN) CHARACTER scalar dummies: direct and
mutual recursion, the dummy passed on whole or as a substring, LEN, TRIM,
keyword actuals, empty strings, and calls in IF and SELECT CASE.

These were rejected: "recursive specialized subroutine calls are not yet
supported", since procedures with CHARACTER dummies are expanded at each call.

Each case is checked at -O0 and -O2 and against gfortran; the interpreter is
too slow for them (see docs/interpreter-todo.md).
Run after cargo build --bin rfort: python tests/test_recursive_character_dummies.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class RecursiveCharacterDummies(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_function_with_optional_dummy(self):
        self.check('''module m
implicit none
contains
recursive function value_of(flag, x, q) result(v)
character(len=*), intent(in) :: flag
real, intent(in) :: x
real, intent(in), optional :: q
real :: v
if (flag == 'both') then
   v = value_of('call', x, q) + value_of('put', x, q)
else if (flag == 'call') then
   v = x
   if (present(q)) v = v + q
else
   v = -x / 2
end if
end function
end module
program main
use m
implicit none
print *, value_of('both', 4.0), value_of('both', 4.0, 1.0), value_of('call', 3.0)
end program
''', "2.0 3.0 3.0\n", interpreter=False)

    def test_call_cycle_only_in_a_disabled_debug_branch(self):
        # `debug .and. ...` with a .false. PARAMETER cannot run, so the call
        # back to the caller in that branch is not a recursion.
        self.check('''module m
implicit none
interface display
   module procedure display_values
end interface
contains
function total(x) result(v)
real, intent(in) :: x(:)
real :: v
logical, parameter :: debug = .false.
v = sum(x)
if (debug .and. v < 0) call display(x)
if (v < 0 .and. .not. (.not. debug)) then
   call display(x)
end if
end function
subroutine display_values(x)
real, intent(in) :: x(:)
print *, total(x)
end subroutine
end module
program main
use m
implicit none
call display([1.0, 2.0])
end program
''', "3.0\n")

    def test_procedures_passing_their_dummies_on(self):
        self.check('''module m
implicit none
contains
recursive function depth(s, n) result(k)
character(len=*), intent(in) :: s
integer, intent(in) :: n
integer :: k
if (n <= 0) then
   k = len(s)
else
   k = depth(s, n - 1) + depth(s(2:), 0)
end if
end function
recursive subroutine show(prefix, n)
character(len=*), intent(in) :: prefix
integer, intent(in) :: n
if (n > 0) call show(n = n - 1, prefix = trim(prefix) // '>')
print '(a,"|",i0)', prefix, len(prefix)
end subroutine
recursive function is_even(label, n) result(t)
character(len=*), intent(in) :: label
integer, intent(in) :: n
logical :: t
if (n == 0) then
   t = label /= ''
else
   t = is_odd(label, n - 1)
end if
end function
recursive function is_odd(label, n) result(t)
character(len=*), intent(in) :: label
integer, intent(in) :: n
logical :: t
if (n == 0) then
   t = .false.
else
   t = is_even(label(1:1), n - 1)
end if
end function
end module
program main
use m
implicit none
character(len=6) :: w
w = 'abcdef'
print *, depth(w, 3), depth('', 2)
call show('x', 2)
if (is_even(w, 4)) print *, 'even 4'
select case (depth('ab', 1))
case (3)
   print *, 'three'
case default
   print *, 'other'
end select
print *, is_even('q', 3), is_odd('q', 3)
end program
''', "21 0\nx>>|3\nx>|2\nx|1\neven 4\nthree\nF T\n", interpreter=False)


    def test_pure_recursion_called_from_a_pure_function(self):
        # Passing the strings is the compiler's own bookkeeping, which PURE
        # procedures may do.
        self.check('''module m
implicit none
contains
pure recursive function pos(v, how) result(r)
integer, intent(in) :: v(:)
character(len=*), intent(in) :: how
integer :: r(size(v))
if (how == 'both') then
   r = pos(pos(v, 'up'), 'down')
else if (how == 'up') then
   r = v + 1
else
   r = v - 2
end if
end function
pure function twice(v) result(r)
integer, intent(in) :: v(:)
integer :: r(size(v))
r = pos(v, 'both')
end function
end module
program main
use m
implicit none
print *, twice([1, 2, 3])
end program
''', "0 1 2\n", interpreter=False)

    def test_dummy_passed_on_to_character_functions(self):
        # Inside the recursive procedure the dummy has a run-time length, so
        # LOWER's result and the constructor comparison are sized at run time.
        self.check('''module m
implicit none
character(len=6), parameter :: names(4) = ['value ','delta ','vega  ','parity']
contains
recursive function greek(flag, name, depth) result(v)
character(len=*), intent(in) :: flag, name
integer, intent(in) :: depth
real :: v
if (depth > 0) then
   v = greek(flag, name, depth-1)
else if (any([names(:2), names(4)] == name)) then
   v = payoff(flag, 10.0, 8.0)
else
   v = -1.0
end if
end function
elemental function payoff(flag, s, x) result(p)
character(len=*), intent(in) :: flag
real, intent(in) :: s, x
real :: p
select case (lower(flag))
case ('c')
   p = max(0.0, s-x)
case ('p')
   p = max(0.0, x-s)
case default
   p = 0.0
end select
end function
elemental function lower(a) result(b)
character(*), intent(in) :: a
character(len(a)) :: b
integer :: i, n
b = a
do i = 1, len(b)
   n = index('ABCDEFGHIJKLMNOPQRSTUVWXYZ', b(i:i))
   if (n /= 0) b(i:i) = 'abcdefghijklmnopqrstuvwxyz'(n:n)
end do
end function
end module
program main
use m
implicit none
print '(4f5.1)', greek('C', 'delta', 2), greek('p', 'parity', 1), greek('P', 'vega', 0), greek('x', 'value', 3)
end program
''', "  2.0  0.0 -1.0  0.0\n", interpreter=False)

if __name__ == "__main__":
    unittest.main()
