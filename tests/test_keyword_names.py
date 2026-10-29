"""Names spelled like keywords (BLOCK, EXIT, IF), partial DATA initialization,
VOLATILE, SAVE module variables and DATA tables in procedures with internal
procedures, from nswc_mod.f90.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_keyword_names.py
"""
import unittest
import test_deallocate


class KeywordNames(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_block_as_dummy_argument_and_array(self):
        self.check("""module m
implicit none
contains
pure subroutine pivot(block, n, total)
integer, intent(in) :: n
real, intent(inout) :: block(n)
real, intent(out) :: total
integer :: j
block(n) = block(1)
total = 0
do j = 1, n
   total = total + block(j)
end do
end subroutine
end module
program p
use m
implicit none
real :: a(3), t
a = [1.0, 2.0, 3.0]
call pivot(a, 3, t)
print *, a, t
block
   integer :: k
   k = 2
   print *, k
end block
end program
""", " 1.0 2.0 1.0 4.0\n 2\n")

    def test_exit_and_if_as_variables(self):
        self.check("""program p
implicit none
logical :: exit
integer :: if, i, n
exit = .false.
if = 3
n = 0
do i = 1, 10
   if (i > if) exit
   n = n + i
end do
if (n == 6) exit = .true.
print *, exit, if, n
do i = 1, 3
   if (exit) then
      n = n + 1
   end if
end do
print *, n
end program
""", " T 3 6\n 9\n")

    def test_partial_data_volatile_and_module_save(self):
        self.check("""module m
implicit none
real, save, volatile :: state
real, save :: counter
contains
subroutine bump()
counter = counter + 1
end subroutine
end module
program p
use m
implicit none
real :: si(16)
integer :: i
data si(8),si(16)/ 1.0e+00,0.0e+00/
do i = 1, 16
   if (i /= 8 .and. i /= 16) si(i) = real(i)
end do
state = 2.5
counter = 0
call bump()
call bump()
print *, si(7), si(8), si(16), state, counter
end program
""", " 7.0 1.0 0.0 2.5 2.0\n")

    def test_data_table_in_procedure_with_internal_function(self):
        self.check("""module m
implicit none
integer, parameter :: sp = kind(1.0)
contains
subroutine s(out)
complex(sp), intent(out) :: out
complex(sp) :: rv(3)
integer :: j
data rv(1)/(-.4,.3)/, rv(2)/(.7,.2)/, rv(3)/(.02,-.06)/
out = (0.0, 0.0)
do j = 1, 3
   out = out + twice(rv(j))
end do
contains
pure function twice(z) result(w)
complex(sp), intent(in) :: z
complex(sp) :: w
w = 2 * z
end function
end subroutine
end module
program p
use m
implicit none
complex :: c
call s(c)
call s(c)
print '(2f8.3)', c
end program
""", "   0.640   0.880\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
