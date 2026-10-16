module stats_mod
implicit none
type :: series
  character(len=:), allocatable :: name
  real(8), allocatable :: v(:)
end type
contains
function mean(s) result(m)
  type(series), intent(in) :: s
  real(8) :: m
  m = sum(s%v)/size(s%v)
end function
end module
program p2
use stats_mod
implicit none
type(series) :: s
s%name = "abc"
s%v = [1d0, 2d0, 3d0, 4d0]
print *, s%name, mean(s)
end program
