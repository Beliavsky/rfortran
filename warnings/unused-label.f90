! Warning:    unused-label
! Enabled by: -Wall
! A statement label that no GO TO, DO, FORMAT reference or I/O branch uses.
program unused_label
implicit none
integer :: i
do i = 1, 3
   write (*, 10) i
end do
20 continue          ! expect: unused-label
10 format ("i = ", i0)
end program unused_label
