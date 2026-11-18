"""OPEN's SIGN=, DELIM= and PAD=, and INQUIRE of them.

SIGN='PLUS' starts each formatted WRITE to the unit as SP does, DELIM= delimits
CHARACTER values in list-directed output, and PAD='NO' ends a formatted READ
that needs more characters than its record holds with IOSTAT=-2.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_open_modes.py
"""
import unittest

import test_deallocate


class OpenModes(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_sign_plus(self):
        self.check("""program p
implicit none
integer :: i, u
character(len=20) :: a, b
open(newunit=u, status='scratch', sign='plus')
write(u, *) 789
write(u, '(i5,f6.2)') 789, 2.5
write(u, '(ss,i5,sp,i5)') 789, 789
write(u, '(i3)', iostat=i) 0
rewind(u)
read(u, *) a
read(u, '(a)') b
print *, trim(a), ' ', trim(b)
read(u, '(a)') b
print *, trim(b)
read(u, '(a)') b
print *, trim(b)
end program p
""", "+789  +789 +2.50\n 789 +789\n +0\n")

    def test_delim_list_output(self):
        self.check("""program p
implicit none
integer :: u
character(len=20) :: line, s, t
open(newunit=u, status='scratch', delim='apostrophe')
write(u, *) "a'b"
rewind(u)
read(u, '(a)') line
print *, trim(line)
rewind(u)
read(u, *) s
print *, trim(s)
close(u)
open(newunit=u, status='scratch', delim='quote')
write(u, *) 'x"y'
write(u, '(a)') 'x"y'
rewind(u)
read(u, *) s
read(u, '(a)') t
print *, trim(s), ' ', trim(t)
end program p
""", "'a''b'\na'b\nx\"y x\"y\n")

    def test_pad_no(self):
        self.check("""program p
implicit none
integer :: ios, i, j
character(len=1) :: line(2)
open(10, pad='no', status='scratch')
write(10, '(a)') '1', '1', '12', 'a'
rewind(10)
read(10, '(i2)', iostat=ios) i
print *, ios
ios = -4321
read(10, '(i1)', iostat=ios) i
print *, ios, i
read(10, '(2i1)', iostat=ios) i, j
print *, ios, i, j
line = 'y'
read(10, '(2a)', iostat=ios, advance='no') line
print *, ios, line
end program p
""", "-2\n0 1\n0 1 2\n-2 ay\n")

    def test_inquire(self):
        self.check("""program p
implicit none
character(len=20) :: s, d, pd
open(10, status='scratch', delim='quote', sign='plus', pad='no')
inquire(10, sign=s, delim=d, pad=pd)
print *, trim(s), ' ', trim(d), ' ', trim(pd)
close(10)
open(10, status='scratch')
inquire(10, sign=s, delim=d, pad=pd)
print *, trim(s), ' ', trim(d), ' ', trim(pd)
close(10)
open(10, status='scratch', form='unformatted')
inquire(10, delim=d, pad=pd)
print *, trim(d), ' ', trim(pd)
close(10)
inquire(6, delim=d, pad=pd)
print *, trim(d), ' ', trim(pd)
end program p
""", "PLUS QUOTE NO\nPROCESSOR_DEFINED NONE YES\nUNDEFINED UNDEFINED\nNONE YES\n")

    def test_rejected_values(self):
        self.reject("""program p
open(10, status='scratch', sign='minus')
end program p
""", "OPEN SIGN= must be 'PLUS', 'SUPPRESS', 'PROCESSOR_DEFINED'")
        self.reject("""program p
open(10, status='scratch', form='unformatted', pad='no')
end program p
""", "OPEN SIGN=, DELIM= and PAD= require a formatted connection")


if __name__ == "__main__":
    unittest.main()
