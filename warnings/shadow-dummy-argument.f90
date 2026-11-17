! Warning:    shadow-dummy-argument
! Enabled by: -Wextra
! A dummy argument with the name of a host variable or named constant, which
! it hides in the procedure.
program shadow_dummy_argument
implicit none
real :: scale_factor
scale_factor = 2.0
print *, scaled(3.0, 4.0)
contains
function scaled(x, scale_factor) result(y)
real, intent(in) :: x
real, intent(in) :: scale_factor         ! expect: shadow-dummy-argument
real :: y
y = x*scale_factor
end function scaled
end program shadow_dummy_argument
