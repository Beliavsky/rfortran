"""Implicitly typed variables in compiled code: assignment, DO variables, READ
destinations, IMPLICIT rules, SAVEd neighbours, procedures and modules.

Compiled code used to give undeclared variables no storage, so a value that
the optimizer could not fold read back as zero. The values below come from
runtime sources (SYSTEM_CLOCK, arguments, input) so that they are not folded.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_implicit_typing.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class ImplicitTyping(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_main_program_runtime_values(self):
        self.check("""program p
integer c
call system_clock(c)
n = 3 + 0*c
k = 0
do i = 1, n
  k = k + i
end do
x = n / 2.0
print *, i, k, x
end program p
""", "4 6 1.5\n")

    def test_procedure_locals(self):
        self.check("""program p
call s(5)
print *, f(4)
contains
subroutine s(n)
integer n
j = n*20
y = j / 8.0
print *, j, y
end subroutine
integer function f(m)
integer m
k = m*m
f = k + 1
end function
end program p
""", "100 12.5\n17\n")

    def test_saved_neighbour(self):
        # A SAVEd or initialized local once hid the implicit variable's value.
        self.check("""program p
call s(4)
call s(5)
end program p
subroutine s(n)
integer n
integer :: calls = 0
calls = calls + 1
j = n*25
print *, calls, j
end subroutine
""", "1 100\n2 125\n")

    def test_implicit_statements(self):
        self.check("""program p
implicit double precision (a-h,o-y)
implicit integer (z)
integer c
call system_clock(c)
x = 1d0/3d0 + 0*c
z = 7 + 0*c
print *, x, z / 2
end program p
""", "0.333333333333333 3\n")

    def test_read_destinations(self):
        self.check("""program p
read *, k, x
j = k + 1
print *, j, x * 2
end program p
""", "8 5.0\n", data="7 2.5\n")

    def test_module_procedure_and_module_variable(self):
        self.check("""module m
integer :: total = 10
contains
subroutine add(n)
integer n
k = n + total
total = k
end subroutine
end module
program p
use m
integer c
call system_clock(c)
call add(5 + 0*c)
print *, total
end program p
""", "15\n")

    def test_implicit_none_still_rejected(self):
        self.reject("""program p
implicit none
integer c
call system_clock(c)
j = c
print *, j
end program p
""", "undefined variable `j`")


if __name__ == "__main__":
    unittest.main()
