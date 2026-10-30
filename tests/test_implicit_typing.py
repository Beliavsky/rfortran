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

    def test_implicit_types_convert_assigned_values(self):
        # I to N are INTEGER and the rest REAL, so values convert on assignment.
        self.check("""program p
integer c
call system_clock(c)
i = 7.9 + 0*c
x = 7 + 0*c
j = i / 2
y = x / 2
m = 2.5
print *, i, x, j, y, m
end program p
""", "7 7.0 3 3.5 2\n")

    def test_implicit_statement_types_in_every_unit(self):
        self.check("""module m
contains
subroutine ms
implicit integer (a-c)
a = 7.9
print *, a
end subroutine
end module
subroutine es
implicit integer (a-c)
b = -7.9
print *, b
end
program p
use m
implicit integer (a-c)
c = 7.9
print *, c
call ms
call es
call cs
contains
subroutine cs
a = 3.5
print *, a
end subroutine
end program p
""", "7\n7\n-7\n3\n")

    def test_declared_integer_assigned_real(self):
        self.check("""program p
integer c, a
double precision d
call system_clock(c)
d = -7.9d0 + 0*c
a = d
print *, a
a = -2.5
print *, a
end program p
""", "-7\n-2\n")

    def test_statement_functions_and_procedure_dummies(self):
        self.check("""program p
real y
external g
real g
f(x) = x**2 + 1
y = f(3.0)
k = f(2.0)
print *, y, k
call t(g, 2.0)
contains
subroutine t(h, a)
print *, h(a)
end subroutine
end program p
real function g(x)
g = x*10
end function
""", "10.0 5\n20.0\n")

    def test_host_association(self):
        # The internal procedure reads and defines implicitly typed host
        # variables, including one the host READs.
        self.check("""program p
integer c
call system_clock(c)
hv = 2.5 + 0*c
read *, n
call s()
print *, hv, n
contains
subroutine s()
k = n * 2
hv = hv + k
n = n + 1
print *, hv, k
end subroutine
end program p
""", "10.5 8\n10.5 5\n", data="4\n")

    def test_host_association_with_implicit_statement(self):
        self.check("""program p
implicit double precision (a-h,o-z)
integer c
call system_clock(c)
x = 1d0/3d0 + 0*c
call s()
contains
subroutine s()
print *, x * 3
end subroutine
end program p
""", "1.0\n")

    def test_conflicting_implicit_statements_rejected(self):
        self.reject("""program p
implicit double precision (a-h,o-z)
implicit integer (z)
end program p
""", "letter `z` already has an IMPLICIT type")
        self.reject("""program p
implicit none
implicit integer (z)
end program p
""", "IMPLICIT NONE cannot appear with other IMPLICIT statements")
        # Different scoping units have their own rules.
        self.check("""subroutine s
implicit integer (a-c)
a = 7.9
print *, a
end
program p
implicit real (a-c)
a = 7.9
print *, a
call s
end program p
""", "7.9\n7\n")

    def test_separate_parameters(self):
        # Constants may be named LEN or KIND, and take their implicit type.
        self.check("""program p
parameter (len = 2, nb = 3, pi = 3.25, kind = 8)
integer a(len)
real z(nb)
a = len
z = pi
print *, a, z, kind, nb * 2
end program p
""", "2 2 3.25 3.25 3.25 8 6\n")
        self.check("""program p
implicit double precision (a-h,o-z)
parameter (half = 0.5d0, k = 7)
character(len=2, kind=1) :: t
t = 'xy'
print *, half * 3, k / 2, t
end program p
""", "1.5 3 xy\n")
        self.reject("""program p
implicit none
parameter (q = 2)
end program p
""", "has no type under IMPLICIT NONE")

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
