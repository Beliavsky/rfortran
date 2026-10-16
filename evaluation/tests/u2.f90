module m
contains
subroutine f(x, y)
  real, intent(in) :: x
  real, intent(out) :: y
  if (x > 0) y = x
end subroutine
end module
program u2
use m
real :: z
call f(-1.0, z)
print *, z
end program
