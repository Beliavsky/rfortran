! Warning:    unchecked-status
! Enabled by: -Wextra
! An IOSTAT or STAT variable that nothing inspects afterwards. Asking for
! the status stops the program from ending on an error, so an unchecked
! status hides the error entirely.
program unchecked_status
implicit none
integer :: ios, stat
real, allocatable :: x(:)
real :: value
read (*, *, iostat=ios) value            ! expect: unchecked-status
allocate (x(10), stat=stat)
if (stat /= 0) error stop "allocation failed"
x = 0.0
print *, sum(x)
end program unchecked_status
