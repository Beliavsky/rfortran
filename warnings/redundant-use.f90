! Warning:    redundant-use
! Enabled by: -Wstyle
! A USE statement, or a name in its ONLY list, that the host or an earlier
! USE already imports, such as a module procedure repeating its module's
! USE.
module stats
use iso_fortran_env, only: dp => real64
implicit none
contains
function mean(x) result(m)
use iso_fortran_env, only: dp => real64      ! expect: redundant-use
real(dp), intent(in) :: x(:)
real(dp) :: m
m = sum(x)/size(x)
end function mean
end module stats

program redundant_use
use stats, only: mean
use iso_fortran_env, only: dp => real64
implicit none
print *, mean([1.0_dp, 2.0_dp])
end program redundant_use
