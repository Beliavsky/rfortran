! Warning:    intrinsic-shadow
! Enabled by: -Wextra
! A procedure with the name of an intrinsic procedure. Where it is visible
! it replaces the intrinsic (here NORM2), which surprises readers who know
! the intrinsic.
program intrinsic_shadow
implicit none
print *, norm2([3.0, 4.0])
print *, trim_spaces("  a  ")
contains
function norm2(x) result(r)                    ! expect: intrinsic-shadow
real, intent(in) :: x(:)
real :: r
r = sqrt(sum(x**2))
end function norm2
function trim_spaces(s) result(r)
character(len=*), intent(in) :: s
character(len=:), allocatable :: r
r = trim(adjustl(s))
end function trim_spaces
end program intrinsic_shadow
