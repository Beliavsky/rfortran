! Warning:    short-circuit
! Enabled by: -Wall
! Fortran does not promise to evaluate .AND. and .OR. left to right or to
! stop early, so `allocated(x) .and. size(x) > 0` may evaluate size(x) for
! an unallocated x. Nest the tests instead.
program short_circuit
implicit none
real, allocatable :: x(:)
if (allocated(x) .and. size(x) > 0) print *, x(1)    ! expect: short-circuit
if (allocated(x)) then
   if (size(x) > 0) print *, x(1)
end if
allocate (x(3))
x = 1.0
print *, sum(x)
end program short_circuit
