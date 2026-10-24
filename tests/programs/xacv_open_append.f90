! Regression for OPEN(..., POSITION='append') in xacv.f90:15203.
! Expected output: first, then second, on separate lines.
! Creates and deletes rfort_open_append_test.tmp in the working directory.
program test_open_append
implicit none
character(len=*), parameter :: filename = 'rfort_open_append_test.tmp'
character(len=20) :: first, second
integer :: ios

open(unit=20, file=filename, status='replace', action='write')
write(20, '(a)') 'first'
close(20)

open(unit=20, file=filename, action='write', position='append', iostat=ios)
if (ios /= 0) error stop 'append OPEN failed'
write(20, '(a)') 'second'
close(20)

open(unit=20, file=filename, status='old', action='read')
read(20, '(a)') first
read(20, '(a)') second
close(20, status='delete')
print '(a)', trim(first), trim(second)
end program test_open_append
