! Repro for INQUIRE(FILE=..., EXIST=...) in xacv.f90:15482.
! Expected output: T followed by F. Run in a scratch directory.
program test_inquire_exist
implicit none
character(len=*), parameter :: filename = 'rfort_inquire_exist_test.tmp'

open(unit=10, file=filename, status='replace')
close(10)
print *, file_exists(filename)
open(unit=10, file=filename, status='old')
close(10, status='delete')
print *, file_exists(filename)

contains

function file_exists(xfile) result(tf)
character(len=*), intent(in) :: xfile
logical :: tf
inquire(file=xfile, exist=tf)
end function file_exists

end program test_inquire_exist
