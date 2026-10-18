module scalar_math
implicit none
private
integer, parameter :: multiplier = 2
public :: twice, powers
contains
pure function twice(i) result(j)
integer, intent(in) :: i
integer :: j
j = multiplier*i
end function twice

pure subroutine powers(i, square, cube)
integer, intent(in) :: i
integer, intent(out) :: square, cube
square = i*i
cube = square*i
end subroutine powers
end module scalar_math

program main
use scalar_math, only: twice, powers
implicit none
integer :: square, cube
call powers(3, square, cube)
print *, twice(3), square, cube
end program main
