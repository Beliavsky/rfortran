"""READ from a CHARACTER array, an array section, a substring or a
deferred-length CHARACTER scalar as the internal file.

Each element of an array is a record, for list-directed READ (formatted READ
from an array is not yet supported). A substring, and a deferred-length
scalar, are read through a copy as one record, by list-directed or formatted
READ. IOSTAT, END= and a module's allocatable array are covered.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_internal_read_records.py
"""
import unittest

import test_deallocate


class InternalReadRecords(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_array_records(self):
        self.check("""program p
implicit none
character(len=10) :: lines(3) = [character(len=10) :: ' 1 2 3', ' 4 5 6', ' 7 8 9']
real(kind=8) :: m(3,3)
integer :: v(4), i, j, ios
read(lines, *, iostat=ios) ((m(i,j), j=1,3), i=1,3)
print *, ios, m(3,3), m(1,2)
read(lines(2:3), *) v
print *, v
end program p
""", "0 9.0 2.0\n4 5 6 7\n")

    def test_end_of_the_array(self):
        self.check("""program p
implicit none
character(len=10) :: a(5)
integer :: i, j, k
a = ''
a(1) = '123'
a(3) = '234'
read(a(1:5:2), *, end=30) i, j, k
print *, 'not reached'
30 print *, i, j
end program p
""", "123 234\n")

    def test_substring_and_deferred_length(self):
        self.check("""program p
implicit none
character(len=8) :: zone
character(len=:), allocatable :: message
integer :: h, m, n
zone = '+0530'
read(zone(2:3), *) h
read(zone(4:5), '(i2)') m
print *, h, m
message = '12 34'
read(message, *) n
print *, n
end program p
""", "5 30\n12\n")

    def test_module_allocatable_array(self):
        self.check("""module bd
implicit none
character(len=14), allocatable, save :: source(:)
end module bd
program p
use bd
implicit none
integer :: x(6), i
allocate(source(2))
source = ['   1   1   1  ', '   4   4   4  ']
read(source, *) (x(i), i=1,6)
print *, x
end program p
""", "1 1 1 4 4 4\n")


if __name__ == "__main__":
    unittest.main()
