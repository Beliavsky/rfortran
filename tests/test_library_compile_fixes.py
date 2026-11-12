"""Constructs from a large NumPy-style library that rfort rejected: ERROR STOP
in a procedure with a dummy named STOP, IEEE_NEXT_AFTER, generic resolution
with HUGE and TRANSPOSE actuals, COMPLEX MERGE and SPREAD, internal
procedures of the same name in several hosts, PURE procedures with shared
host locals, section actuals, COMPLEX literals, TRANSFER and BIND(C)
interfaces, a recursive call passing a local ALLOCATABLE array, and OPEN with
a deferred-length FILE=."""
import unittest

import test_deallocate


class LibraryCompileFixes(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_error_stop_with_dummy_named_stop(self):
        self.check("""module m
implicit none
contains
pure function count_to(start, stop) result(n)
integer, intent(in) :: start, stop
integer :: n
if (stop < start) error stop 'count_to: stop before start'
n = stop - start + 1
end function count_to
end module m
program main
use m
implicit none
print '(i0)', count_to(2, 5)
end program main
""", "4\n")

    def test_ieee_next_after_and_typed_generic_actuals(self):
        self.check("""module m
implicit none
interface solve
   module procedure solve_real
end interface
contains
function solve_real(a, b) result(x)
real(kind=8), intent(in) :: a(:,:), b(:)
real(kind=8), allocatable :: x(:)
integer :: i
allocate(x(size(b)))
do i = 1, size(b)
   x(i) = b(i) / a(i, i)
end do
end function solve_real
end module m
program main
use m
use, intrinsic :: ieee_arithmetic, only: ieee_next_after
implicit none
real(kind=8) :: x = 1.0d0, v(2,2), y(2)
v = reshape([2.0d0, 0.0d0, 0.0d0, 4.0d0], [2, 2])
y = [2.0d0, 4.0d0]
print '(es24.16)', ieee_next_after(x, huge(x)) - x
print '(es24.16)', ieee_next_after(x, -huge(x)) - x
print '(f6.3)', ieee_next_after(x, x)
print '(2f8.4)', solve(transpose(v), y)
end program main
""", """  2.2204460492503131E-16
 -1.1102230246251565E-16
 1.000
  1.0000  1.0000
""")

    def test_complex_merge_spread_and_same_named_internal_functions(self):
        self.check("""module m
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
pure elemental function cmax(a, b) result(r)
complex(kind=dp), intent(in) :: a, b
complex(kind=dp) :: r
r = merge(b, a, real(a) < real(b))
end function cmax
function twice_real(a) result(c)
real(kind=dp), intent(in) :: a(:)
real(kind=dp), allocatable :: c(:)
c = scaled(a)
contains
pure function scaled(x) result(y)
real(kind=dp), intent(in) :: x(:)
real(kind=dp), allocatable :: y(:)
y = 2 * x
end function scaled
end function twice_real
function triple_complex(a) result(c)
complex(kind=dp), intent(in) :: a(:)
complex(kind=dp), allocatable :: c(:)
c = scaled(a)
contains
pure function scaled(x) result(y)
complex(kind=dp), intent(in) :: x(:)
complex(kind=dp), allocatable :: y(:)
y = 3 * x
end function scaled
end function triple_complex
end module m
program main
use m
implicit none
complex(kind=dp) :: z(2), s(3, 2), row(2)
z = cmax([(1.0_dp, 5.0_dp), (4.0_dp, 0.0_dp)], [(2.0_dp, 0.0_dp), (3.0_dp, 1.0_dp)])
print '(4f5.1)', z
s = spread(z, dim=1, ncopies=3)
row = s(3, :)
print '(4f5.1)', row
print '(2f5.1)', twice_real([1.0_dp, 2.0_dp])
z = triple_complex([(1.0_dp, 1.0_dp), (0.0_dp, 2.0_dp)])
print '(4f5.1)', z
end program main
""", """  2.0  0.0  4.0  0.0
  2.0  0.0  4.0  0.0
  2.0  4.0
  3.0  3.0  0.0  6.0
""")

    def test_pure_procedures(self):
        self.check("""module m
use, intrinsic :: iso_fortran_env, only: int64
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
pure integer function median_index(x) result(n)
real(kind=dp), intent(in) :: x(:)
real(kind=dp), allocatable :: s(:)
s = x
call sort(s(1:size(s)))
n = nint(quantile(0.5_dp))
contains
pure elemental real(kind=dp) function quantile(q)
real(kind=dp), intent(in) :: q
quantile = s(1 + int(q * (size(s) - 1)))
end function quantile
end function median_index
pure subroutine sort(x)
real(kind=dp), intent(inout) :: x(:)
integer :: i, j
real(kind=dp) :: t
do i = 2, size(x)
   t = x(i)
   j = i - 1
   do while (j >= 1)
      if (x(j) <= t) exit
      x(j + 1) = x(j)
      j = j - 1
   end do
   x(j + 1) = t
end do
end subroutine sort
pure elemental logical function signbit(x)
real(kind=dp), intent(in) :: x
integer(kind=int64) :: bits
bits = transfer(x, bits)
signbit = bits < 0_int64
end function signbit
pure function zeros(n) result(c)
integer, intent(in) :: n
complex(kind=dp), allocatable :: c(:)
allocate(c(n), source=(0.0_dp, 0.0_dp))
end function zeros
end module m
program main
use m
implicit none
real(kind=dp) :: v(3)
logical :: negative(3)
print '(i0)', median_index([7.0_dp, 3.0_dp, 5.0_dp])
v = [-0.0_dp, 1.0_dp, -2.0_dp]
negative = signbit(v)
print '(3l2)', negative
print '(i0)', size(zeros(4))
end program main
""", "5\n T F T\n4\n")

    def test_recursive_call_with_local_allocatable(self):
        self.check("""module m
implicit none
contains
subroutine number(idx)
integer, intent(out) :: idx(:)
integer, allocatable :: tmp(:)
idx = 0
allocate(tmp(size(idx)))
call fill(idx, tmp, 1)
end subroutine number
recursive subroutine fill(idx, tmp, lo)
integer, intent(inout) :: idx(:), tmp(:)
integer, intent(in) :: lo
if (lo > size(idx)) return
tmp(lo) = 10 * lo
idx(lo) = tmp(lo) + lo
call fill(idx, tmp, lo + 1)
end subroutine fill
end module m
program main
use m
implicit none
integer :: idx(3)
call number(idx)
print '(3i4)', idx
end program main
""", "  11  22  33\n")

    def test_open_with_deferred_length_file(self):
        self.check("""program main
implicit none
character(len=:), allocatable :: stem
integer :: u, stat
stem = 'rfort_deferred_open'
open(newunit=u, file=stem // '.txt', status='replace', iostat=stat)
print '(i0)', stat
close(u, status='delete')
open(newunit=u, file=stem, status='replace', action='write', iostat=stat)
print '(i0)', stat
close(u, status='delete')
end program main
""", "0\n0\n")


if __name__ == '__main__':
    unittest.main()
