"""Statement forms and I/O control: FORMAT statements, DO CONCURRENT, DO with
a terminal label, DATA, FLUSH, ENDFILE, IOSTAT and IOMSG on file statements,
formatted file output with implied-DO lists, formatted READ with edit
descriptors, continuation of strings and names, and keywords used as names.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_statement_forms_io.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class StatementFormsIo(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_format_statements(self):
        """Labeled FORMAT statements used by PRINT, WRITE and READ."""
        self.check("""program p
real :: x = 1.5, y = -2.25
integer :: i = 7
print 10, x, y, i
write(*, 20) 'X=', x
10 format (2E16.3,1x,I0)
20 format( a, f5.2, &
   & ' done')
end program
""", '       0.150E+01      -0.225E+01 7\nX= 1.50 done\n')
        self.check("""program p
integer :: k, m
read(*, 30) k
read 30, m
print 40, k + m
30 format(i3)
40 format('sum=', i0)
end program
""", 'sum=42\n', data=' 12\n 30\n')

    def test_do_concurrent_and_labeled_do(self):
        """DO CONCURRENT with strides and a mask, and DO with a terminal label."""
        self.check("""program p
integer :: i, a(5)
do concurrent (i = 1:5)
  a(i) = i*i
end do
do concurrent (i = 1:5, a(i) > 4)
  a(i) = -a(i)
end do
print *, a
end program
""", ' 1 4 -9 -16 -25\n')
        self.check("""program p
integer :: i, s
s = 0
do 100 i = 1, 5
  s = s + i
100 continue
do 200, i = 1, 3
200 s = s + 1
print *, s
end program
""", ' 18\n', std="legacy")
        self.check("""program p
integer :: i, j, s
s = 0
do 10 i = 1, 3
  do 10 j = 1, 2
    if (j == 2) cycle
    s = s + 10*i + j
10 continue
print *, s
end program
""", ' 63\n', std="legacy")

    def test_data_statements(self):
        """DATA with repeat counts, BOZ values and arrays."""
        self.check("""program p
integer :: a, b, n(3)
real :: c(4)
data a / z'f' /, b / 3 /
data n / 1, 2, 3 /, c / 2*1.0, 2*0.5 /
print *, a, b, n, c
end program
""", ' 15 3 1 2 3 1.0 1.0 0.5 0.5\n')

    def test_file_statements_and_iostat(self):
        """FLUSH, ENDFILE, REWIND, CLOSE and OPEN with IOSTAT and IOMSG."""
        self.check("""program p
use, intrinsic :: iso_fortran_env, only: stdout => output_unit
integer :: ios, lun, v
character(len=100) :: msg
flush(stdout)
flush stdout
flush (stdout, iostat=ios, iomsg=msg)
open(newunit=lun, file='f5.tmp', status='replace', iostat=ios, iomsg=msg)
write(lun, *) 42
rewind (lun, iostat=ios, iomsg=msg)
read(lun, *, iostat=ios, iomsg=msg) v
rewind lun
endfile lun
backspace lun
close(lun, iostat=ios, status='delete')
print *, v, ios
end program
""", ' 42 0\n')
        self.check("""program p
integer :: ios, lun, v
character(len=100) :: msg
open(newunit=lun, file='no_such_dir/x.txt', status='old', action='read', iostat=ios, iomsg=msg)
if (ios /= 0) then
  print *, 'open failed as expected'
else
  print *, 'unexpected success'
end if
open(20, file='f12.tmp', status='replace', iostat=ios)
print *, ios
write(20, *) 7
rewind(20)
read(20, *, iostat=ios, iomsg=msg) v
print *, v, ios
close(20, status='delete', iostat=ios)
print *, ios
end program
""", ' open failed as expected\n 0\n 7 0\n 0\n')

    def test_formatted_file_output_and_input(self):
        """Implied-DO formatted WRITE to files, and formatted READ with edit descriptors."""
        self.check("""program p
integer :: i, j, lun
character(len=80) :: line
open(10, file='f21.tmp', status='replace')
do i = 1, 3
  write(10,'(a,i3,*(i3))') 'line ', i, (j, j=1,i)
end do
rewind(10)
do i = 1, 3
  read(10,'(a)') line
  print '(a)', trim(line)
end do
close(10, status='delete')
open(newunit=lun, file='f21b.tmp', status='replace')
write(lun,'(i0)') (100+i, i=1,3)
close(lun, status='delete')
lun = 6
write(lun,'(*(i4))') (j*j, j=1,4)
end program
""", 'line   1  1\nline   2  1  2\nline   3  1  2  3\n   1   4   9  16\n')
        self.check("""program p
integer :: i, j, ios, v(3)
real :: x, y
character(len=4) :: name
logical :: flag
open(10, file='f22.tmp', status='replace')
write(10,'(a)') 'abcd  12   345 T'
write(10,'(a)') ' 1.5  250 3.25e1'
write(10,'(a)') '  7  8  9'
write(10,'(a)') '      '
rewind(10)
read(10,'(a4,2x,i2,i6,l2)') name, i, j, flag
print *, name, i, j, flag
read(10,'(f4.1,f5.2,e7.0)') x, y, x
print *, y, x
read(10,'(3i3)') v
print *, v
read(10,'(i3)', iostat=ios) i
print *, i, ios
read(10,'(i3)', iostat=ios) i
print *, ios /= 0
close(10, status='delete')
end program
""", ' abcd 12 345 T\n 2.5 32.5\n 7 8 9\n 0 0\n T\n')

    def test_continuation_and_names(self):
        """Comment lines in continued strings, names split across lines, and CASE and SELECT as names."""
        self.check("""program p
character(len=:), allocatable :: s
character(len=40) :: t
t = 'no comment allowed &
   &on the end'
print *, trim(t)
t='&
   &abc&
   &def'
print *, trim(t)
end program
""", ' no comment allowed on the end\n abcdef\n')
        self.check("""program p
character(len=200) :: string
         string='no comment allowed &
            &on the end of a continued string &
            ! keep going ...
            & but comment lines are allowed between ' ! but can go on the end
print *, trim(string)
end program
""", ' no comment allowed on the end of a continued string  but comment lines are allowed between\n')
        self.check("""program p
character(len=200) :: string
string='a &
   &b &
   ! keep
   & c'
print *, trim(string)
end program
""", ' a b  c\n')
        self.check("""program p
integer :: napier_constant = 3
write(*,*) napier_&
  &constant, 12&
  &34
end program
""", ' 3 1234\n')
        self.check("""program p
integer :: case = 0, select = 1
case = case + select
print *, case
end program
""", ' 1\n')
        self.check("""program p
integer :: i
outer: select case (3)
case (1:2)
  print *, 'low'
case default
  print *, 'high'
end select outer
do i = 1, 2
  select case (i)
  case (1); print *, 'one'
  case default; print *, 'other'
  end select
end do
end program
""", ' high\n one\n other\n')

    def test_dummies_without_intent(self):
        """A dummy without INTENT may be defined by the call, or receive an expression."""
        self.check("""program p
integer :: lun
call makefile(lun)
print *, lun > 0 .or. lun < 0
contains
subroutine makefile(lun)
integer :: lun
open(newunit=lun, status='scratch')
end subroutine
end program
""", ' T\n')
        self.check("""program p
integer :: k
k = 1
call show(k + 1)
call show(k)
contains
subroutine show(n)
integer :: n
print *, n
end subroutine
end program
""", ' 2\n 1\n')


if __name__ == "__main__":
    unittest.main(verbosity=2)
