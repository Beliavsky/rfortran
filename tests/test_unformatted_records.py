"""Records of unformatted files.

- A sequential unformatted record is written as gfortran writes it, with its
  length in bytes before and after it, so BACKSPACE can step back over it,
  a READ of fewer items skips the rest of the record, and a READ of more is
  an error.
- A sequential WRITE ends the file after its record.
- END= and ERR= work on unformatted READ, as IOSTAT= did.
- ACCESS='DIRECT' with RECL=, and REC= on READ and WRITE.
- RECL= limits the length of a sequential record.
- ENDFILE connects an unconnected unit to fort.N, as a data transfer does.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_unformatted_records.py
"""
import unittest

import test_deallocate


class UnformattedRecords(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_sequential_records(self):
        self.check("""program p
implicit none
integer :: a(3), b(3), n, ios
open(11, file='rfort_sequential_records.bin', form='unformatted', status='replace')
a = [1, 2, 3]
write(11) a
write(11) a*10
write(11) a*100
rewind(11)
! A sequential WRITE ends the file after its record.
write(11) a+1
rewind(11)
read(11) b
print *, b
read(11, end=10) b
print *, 'not reached'
10 print *, 'end'
! The first BACKSPACE steps back over the end of the file.
backspace(11)
backspace(11)
read(11) b
print *, b
rewind(11)
! A READ of fewer items skips the rest of the record.
read(11) n
read(11, iostat=ios) n
print *, n, ios < 0
close(11)
! Each record has its length in bytes before and after it.
open(12, file='rfort_sequential_records.bin', access='stream', form='unformatted', status='old')
read(12) n
print *, n
close(12, status='delete')
end program p
""", "2 3 4\nend\n2 3 4\n2 T\n12\n")

    def test_reading_past_the_record(self):
        self.check("""program p
implicit none
integer :: a(2), b(3), ios
a = [1, 2]
open(11, status='scratch', form='unformatted')
write(11) a
rewind(11)
read(11, iostat=ios) b
print *, ios > 0
rewind(11)
read(11, err=20) b
print *, 'not reached'
20 print *, 'error branch'
rewind(11)
read(11) b(1:2)
print *, b(1:2)
end program p
""", "T\nerror branch\n1 2\n")

    def test_end_of_an_empty_file(self):
        self.check("""program p
implicit none
integer :: i
open(13, status='scratch', form='unformatted')
endfile(13)
rewind(13)
read(13, end=30) i
print *, 'not reached'
30 print *, 'end'
end program p
""", "end\n")

    def test_direct_access(self):
        self.check("""program p
implicit none
integer :: i, v, ios
real :: x
open(10, status='scratch', access='direct', recl=8)
do i = 3, 1, -1
   write(10, rec=i) i, real(i)/2
end do
read(10, rec=2) v, x
print *, v, x
read(10, rec=3) v
print *, v
read(10, rec=1) v, x
print *, v, x
! Twelve bytes do not fit in a record of eight.
write(10, rec=2, iostat=ios) 1, 2, 3
print *, ios > 0
read(10, rec=2, iostat=ios) v, x, i
print *, ios > 0
close(10)
end program p
""", "2 1.0\n3\n1 0.5\nT\nT\n")

    def test_direct_access_named_file(self):
        self.check("""program p
implicit none
character(len=4) :: word
integer :: u
open(newunit=u, file='rfort_direct_records.bin', access='direct', recl=4, form='unformatted', status='replace')
write(u, rec=2) 'bcde'
write(u, rec=1) 'abcd'
close(u)
open(newunit=u, file='rfort_direct_records.bin', access='direct', recl=4, status='old')
read(u, rec=2) word
print *, word
read(u, rec=1) word
print *, word
close(u, status='delete')
end program p
""", "bcde\nabcd\n")

    def test_recl_limits_sequential_records(self):
        self.check("""program p
implicit none
real :: x(5)
integer :: ios
x = 1.0
open(14, status='scratch', form='unformatted', access='sequential', recl=16)
write(14, iostat=ios) x(1:4)
print *, ios
write(14, iostat=ios) x
print *, ios /= 0
end program p
""", "0\nT\n")

    def test_direct_access_requires_recl(self):
        self.reject("""program p
open(10, status='scratch', access='direct')
end program p
""", "ACCESS='DIRECT' requires RECL")


if __name__ == "__main__":
    unittest.main()
