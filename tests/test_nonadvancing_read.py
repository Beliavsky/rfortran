"""Non-advancing formatted READ: ADVANCE='NO', with SIZE=, EOR= and PAD=.

A non-advancing READ leaves the file after the characters it read, within
the record, so the next READ continues there. When the record ends before
the format's fields do, the fields are padded with blanks (unless PAD='NO'),
the end of the record is consumed, and IOSTAT is IOSTAT_EOR (EOR= branches).
SIZE= counts the characters that data edit descriptors read.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_nonadvancing_read.py
"""
import unittest

import test_deallocate


class NonadvancingRead(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_continuing_within_a_record(self):
        self.check("""program p
implicit none
integer :: i, j, k
character(len=3) :: c
open(10, status='scratch')
write(10, '(a)') '123456789'
write(10, '(a)') 'next'
rewind(10)
read(10, '(i2)', advance='no') i
read(10, '(i3)', advance='no') j
read(10, '(a)', advance='no') c
read(10, '(i1)') k
print *, i, j, c, k
read(10, '(a)') c
print *, c
end program p
""", "12 345 678 9\nnex\n")

    def test_end_of_record_size_and_padding(self):
        self.check("""program p
use, intrinsic :: iso_fortran_env, only: iostat_eor, iostat_end
implicit none
integer :: i1, i2, n, ios
character(len=8) :: word
open(11, status='scratch')
write(11, '(a)') '1 3'
write(11, '(a)') 'abc'
rewind(11)
i1 = 7
i2 = 7
read(11, '(2i2)', advance='no', iostat=ios, size=n) i1, i2
print *, i1, i2, n, ios == iostat_eor
read(11, '(a)', advance='no', iostat=ios, size=n) word
print *, '[', word, ']', n, ios == iostat_eor
read(11, '(a)', advance='no', iostat=ios) word
print *, ios == iostat_end
end program p
""", "1 3 3 T\n[abc     ] 3 T\nT\n")

    def test_eor_branch(self):
        self.check("""program p
implicit none
character(len=1) :: c
integer :: count
open(12, status='scratch')
write(12, '(a)') 'xyz'
rewind(12)
count = 0
do
   read(12, '(a1)', advance='no', eor=20) c
   count = count + 1
end do
20 print *, count
end program p
""", "3\n")

    def test_standard_input(self):
        self.check("""program p
implicit none
integer :: a, b
read(*, '(i3)', advance='no') a
read(*, '(i3)') b
print *, a + b
end program p
""", "123\n", data=" 50 73\n")

    def test_size_requires_nonadvancing(self):
        self.reject("""program p
integer :: i, n
read(*, '(i2)', size=n) i
end program p
""", "SIZE= and EOR= require ADVANCE='NO'")

    def test_list_directed_is_rejected(self):
        self.reject("""program p
integer :: i
read(*, *, advance='no') i
end program p
""", "ADVANCE='NO' requires a format other than *")


if __name__ == "__main__":
    unittest.main()
