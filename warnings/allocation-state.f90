! Warning:    allocation-state
! Enabled by: default
! DEALLOCATE of an ALLOCATABLE that is not allocated, or ALLOCATE of one
! that already is, on every path to the statement. Either stops the program
! with an error unless STAT= is given.
program allocation_state
implicit none
real, allocatable :: x(:), y(:)
allocate (x(3))
x = 1.0
allocate (x(5))          ! expect: allocation-state
deallocate (y)           ! expect: allocation-state
if (allocated(x)) deallocate (x)
allocate (x(5))
print *, size(x)
end program allocation_state
