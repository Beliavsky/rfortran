"""INQUIRE of a unit's connection: ACCESS, FORM, BLANK, RECL, NEXTREC, POS.

Also a formatted WRITE to a file whose format ends with a slash, which writes
an empty record; the interpreter dropped it.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_inquire_connection.py
"""
import unittest

import test_deallocate


class InquireConnection(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_direct_access(self):
        self.check("""program p
implicit none
integer :: n, r
character(len=12) :: access, form
open(9, status='scratch', access='direct', recl=80, form='unformatted')
inquire(9, access=access, form=form, recl=r, nextrec=n)
print *, trim(access), ' ', trim(form), r, n
write(9, rec=5) 1
inquire(9, nextrec=n)
print *, n
read(9, rec=1) r
inquire(9, nextrec=n)
print *, n
end program p
""", "DIRECT UNFORMATTED 80 1\n6\n2\n")

    def test_sequential_and_stream(self):
        self.check("""program p
implicit none
integer :: pos, u
character(len=12) :: access, form, blank
open(newunit=u, status='scratch')
inquire(u, access=access, form=form, blank=blank)
print *, trim(access), ' ', trim(form), ' ', trim(blank)
close(u)
open(newunit=u, status='scratch', access='stream', form='unformatted')
write(u) 1, 2
inquire(u, access=access, pos=pos)
print *, trim(access), pos
inquire(42, access=access, form=form)
print *, trim(access), ' ', trim(form)
end program p
""", "SEQUENTIAL FORMATTED NULL\nSTREAM 9\nUNDEFINED UNDEFINED\n")

    def test_slash_writes_an_empty_record(self):
        self.check("""program p
implicit none
integer :: i(4), j, ios
open(10, status='scratch')
write(10, '(2(i4/)/2(i6/))') (j, j=1,4)
rewind(10)
i = 0
! The READ needs the empty record after the last value.
read(10, '(2(i4/)/2(i6/))', iostat=ios) i
print *, ios, i
end program p
""", "0 1 2 3 4\n")

    def test_by_file_is_rejected(self):
        self.reject("""program p
character(len=12) :: access
inquire(file='x.dat', access=access)
end program p
""", "INQUIRE by FILE does not yet support ACCESS")


if __name__ == "__main__":
    unittest.main()
