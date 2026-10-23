"""Unformatted stream READ and WRITE: OPEN with ACCESS='STREAM' and
FORM='UNFORMATTED', values transferred as the bytes of their kinds, arrays in
array element order, derived-type objects component by component, runtime
CHARACTER arrays, POS= and IOSTAT at end of file.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_stream_io.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class StreamIO(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_round_trip_of_size_and_array(self):
        """ofort's xstream.f90, with values exact in single precision."""
        self.check("""implicit none
character (len=*), parameter :: xfile = "rfort_stream_1.bin"
integer, parameter :: iu = 20
real(kind=8) :: i(3)
real(kind=8), allocatable :: ichk(:)
integer :: isize
i = [10.5, 20.25, 30.125]
open (unit=iu, file=xfile, access="stream", form="unformatted", action="write", status="replace")
write (iu) size(i), i
close(unit=iu)
open (unit=iu, file=xfile, access="stream", form="unformatted", action="read", status="old")
read (iu) isize
allocate (ichk(isize))
read (iu) ichk
close (iu, status="delete")
print '(3f8.3)', ichk
print *, isize, sum(abs(i-ichk))
end
""", "  10.500  20.250  30.125\n 3 0.0\n")

    def test_byte_layout_of_kinds(self):
        """The bytes of each kind, little-endian, read back one at a time."""
        self.check("""implicit none
integer :: iu, n, k
integer(kind=1) :: b
open (newunit=iu, file="rfort_stream_2.bin", access="stream", form="unformatted", status="replace")
write (iu) 258, -2_2, 3_8, .true., 1.5, -2.0d0, "ab"
n = 4 + 2 + 8 + 4 + 4 + 8 + 2
rewind (iu)
do k = 1, n
   read (iu) b
   write (*, "(i0,1x)", advance="no") b
end do
print *
close (iu, status="delete")
end
""", "2 1 0 0 -2 -1 3 0 0 0 0 0 0 0 1 0 0 0 0 0 -64 63 0 0 0 0 0 0 0 -64 97 98\n")

    def test_runtime_arrays_kinds_and_end_of_file(self):
        """SHAPE, runtime CHARACTER and REAL arrays, integer kinds, LOGICAL,
        CHARACTER, an implied DO, REWIND and IOSTAT at end of file."""
        self.check("""implicit none
integer, parameter :: n1 = 3
character(len=10), allocatable :: names(:), names2(:)
real, allocatable :: x(:,:), y(:,:)
integer :: iu = 10, m1, m2, i, ios, k
integer(kind=2) :: small
integer(kind=8) :: big
logical :: flag
character(len=5) :: word
real(kind=8) :: d(4)
k = 2
allocate(names(k), x(n1,k))
names = ["x1", "x2"]
do i = 1, k
  x(:,i) = [(real(m1+3*(i-1)), m1=1,n1)]
end do
open (unit=iu, file="rfort_stream_3.bin", form="unformatted", access="stream", status="replace")
write (iu) shape(x), names, x
write (iu) -7_2, 123456789012_8, .true., "hello", (2.5d0*i, i=1,4)
rewind (iu)
read (iu) m1, m2
allocate (y(m1,m2), names2(m2))
read (iu) names2, y
read (iu) small, big, flag, word, d
print *, m1, m2, all(x == y), all(names == names2), trim(names2(2))
print *, small, big, flag, word, d
read (iu, iostat=ios) m1
print *, ios < 0
close (iu, status="delete")
end
""", " 3 2 T T x2\n -7 123456789012 T hello 2.5 5.0 7.5 10.0\n T\n")

    def test_derived_type_array_and_position(self):
        """ofort's xstream_pos_dt.f90: an array of derived type, then READ with
        POS= into a derived-type scalar."""
        self.check("""module stream_pos_dt_mod
implicit none
type :: rec
   integer :: i
   real :: r(2)
end type rec
end module stream_pos_dt_mod

program main
use stream_pos_dt_mod, only: rec
implicit none
integer, parameter :: iu = 10
type(rec) :: x(3), y
integer :: i
do i = 1, 3
   x(i)%i = 10*i
   x(i)%r = [real(i) + 0.25, real(i) + 0.5]
end do
open(unit=iu, file="rfort_stream_4.bin", action="readwrite", access="stream", form="unformatted", status="replace")
write(iu) x
read(iu, pos=13) y
close (iu, status="delete")
print *, y%i, y%r
end program main
""", " 20 2.25 2.5\n")

    def test_unsupported_controls(self):
        self.reject("""integer :: iu, n
open (newunit=iu, file="x.bin", access="stream", form="unformatted")
read (iu, end=10) n
10 continue
end
""", "unformatted READ currently supports UNIT, IOSTAT, IOMSG and POS")
        self.reject("""integer :: n
read (*, *, pos=3) n
end
""", "POS requires an unformatted stream READ")


if __name__ == "__main__":
    unittest.main(verbosity=2)
