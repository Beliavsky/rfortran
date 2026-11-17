! Warning:    could-be-function
! Enabled by: -Weverything
! A subroutine whose only effect is to define one INTENT(OUT) argument. As a
! PURE function it can be used in expressions, and the result is clearly
! its output.
program could_be_function
implicit none
real :: a
call circle_area(2.0, a)
print *, a, area(2.0)
contains
subroutine circle_area(r, a)              ! expect: could-be-function
real, intent(in) :: r
real, intent(out) :: a
a = 3.14159*r*r
end subroutine circle_area
pure function area(r) result(a)
real, intent(in) :: r
real :: a
a = 3.14159*r*r
end function area
end program could_be_function
