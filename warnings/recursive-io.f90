! Warning:    recursive-io
! Enabled by: default
! An I/O statement in a function that is referenced by another I/O statement
! on the same unit. Such recursive I/O is not allowed, and gfortran stops
! with "Recursive I/O not allowed". Compute the value first.
program recursive_io
implicit none
real :: y
y = noisy_square(3.0)
print *, y
print *, noisy_square(2.0)
contains
function noisy_square(x) result(s)
real, intent(in) :: x
real :: s
print *, "squaring", x       ! expect: recursive-io
s = x*x
end function noisy_square
end program recursive_io
