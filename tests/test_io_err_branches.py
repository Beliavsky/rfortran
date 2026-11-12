"""ERR=, END= and EOR= with other I/O controls; IOSTAT=, IOMSG= and ERR= on
formatted WRITE; ERR= on OPEN, CLOSE and REWIND."""
import unittest

import test_deallocate


class IoErrBranches(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_read_err_in_procedure_and_with_iostat_and_end(self):
        self.check("""module m
implicit none
contains
subroutine get(iu, n, ok)
integer, intent(in) :: iu
integer, intent(out) :: n
logical, intent(out) :: ok
ok = .true.
read (iu, *, err=20) n
return
20 ok = .false.
n = -99
end subroutine get
end module m
program p
use m
implicit none
integer :: n, ios, iu
logical :: ok
real :: x
character(len=20) :: s
open (newunit=iu, file="err_a.txt", status="replace")
write (iu, '(a)') "12"
write (iu, '(a)') "abc"
write (iu, '(a)') "3.5"
close (iu)
open (newunit=iu, file="err_a.txt", status="old", err=90)
call get(iu, n, ok)
print *, "get:", n, ok
call get(iu, n, ok)
print *, "get:", n, ok
read (iu, *, iostat=ios, err=30, end=40) x
print *, "x =", x, ios
read (iu, *, iostat=ios, err=30, end=40) x
print *, "not reached"
30 print *, "err branch"
go to 45
40 print *, "end branch, ios /= 0:", ios /= 0
45 continue
rewind (iu, err=55)
read (iu, '(i5)', end=50, err=55) n
print *, "after rewind", n
50 continue
55 continue
close (iu, status="delete", err=60)
60 continue
open (newunit=iu, file="no_such_dir/x.txt", status="old", err=70)
print *, "opened"
70 print *, "open err branch"
write (s, '(i0)', iostat=ios) 42
print *, trim(s), ios
write (*, *, iostat=ios, err=80) "list write", 1
80 print *, "ios", ios
stop
90 print *, "first open failed"
end program p
""", """ get: 12 T
 get: -99 F
 x = 3.50000000 0
 end branch, ios /= 0: T
 after rewind 12
 open err branch
 42 0
 list write 1
 ios 0
""")

    def test_branches_leave_a_loop(self):
        self.check("""program b
implicit none
integer :: iu, n, total, k
open (newunit=iu, file="err_b.txt", status="replace")
write (iu, '(a)') "1", "2", "3", "x", "5"
close (iu)
open (newunit=iu, file="err_b.txt", status="old")
total = 0
do k = 1, 10
   read (iu, *, err=10, end=20) n
   total = total + n
end do
10 print *, "bad value after", total
read (iu, *, err=10, end=20) n
print *, "next", n
read (iu, *, err=10, end=20) n
20 print *, "end, total", total
close (iu, status="delete")
end program b
""", """ bad value after 6
 next 5
 end, total 6
""")

    def test_end_of_file_without_end_or_iostat_stops(self):
        # ERR= covers errors only; an end of file still stops the program.
        self.check("""program c
implicit none
integer :: iu, n
open (newunit=iu, status="scratch")
write (iu, '(a)') "7"
rewind (iu)
read (iu, *, err=10) n
print *, n
read (iu, *, err=10) n
print *, "not reached"
10 print *, "not reached either"
end program c
""", error="end of file during READ")

    def test_unformatted_err(self):
        self.check("""program d
implicit none
integer :: iu, n, ios
open (newunit=iu, file="err_d.bin", form="unformatted", status="replace")
write (iu, err=10) 5
rewind (iu)
read (iu, err=10) n
print *, n
read (iu, iostat=ios, end=20, err=10) n
10 print *, "not reached"
20 print *, "end", ios /= 0
close (iu, status="delete")
end program d
""", """ 5
 end T
""")

    def test_write_iostat_iomsg_and_err(self):
        # An explicit format reports a bad format in IOSTAT= and takes ERR=;
        # list-directed and internal output set IOSTAT= to zero.
        self.check("""program w
implicit none
integer :: iu, ios, n = 42, st(2)
character(len=20) :: s, msg, fmt
open (newunit=iu, file="err_w.txt", status="replace")
write (iu, '(i0)', iostat=ios) n
print *, "file", ios
write (iu, *, iostat=ios, iomsg=msg) n, "x"
print *, "list file", ios
write (*, '(a,i0)', iostat=ios) "stdout ", n
print *, "stdout", ios
write (s, '(i0)', iostat=ios) n
print *, "internal ", trim(s), ios
write (s, *, iostat=st(2)) n
print *, "internal list", st(2)
fmt = "(i0"
msg = "unset"
write (iu, fmt, iostat=ios, iomsg=msg) n
print *, "bad format nonzero:", ios /= 0
write (iu, fmt, err=10) n
print *, "not reached"
10 print *, "err branch"
write (iu, '(i0)', err=20, iostat=ios) n
print *, "no error", ios
20 close (iu, status="delete")
end program w
""", """ file 0
 list file 0
stdout 42
 stdout 0
 internal 42 0
 internal list 0
 bad format nonzero: T
 err branch
 no error 0
""")


if __name__ == '__main__':
    unittest.main()
