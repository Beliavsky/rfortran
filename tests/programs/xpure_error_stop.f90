! Reproducer for the ERROR STOP guard in util.f90: compress.
! Expected output: 1 1
program test_pure_error_stop
implicit none
integer :: factors(2)
call compress(2, factors)
print *, factors
contains
pure subroutine compress(n, factors)
integer, intent(in) :: n
integer, intent(out) :: factors(:)
if (size(factors) /= n) error stop 'in compress, incorrect factors size'
factors = 1
end subroutine compress
end program test_pure_error_stop
