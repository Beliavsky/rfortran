module pure_functions
implicit none
contains
pure function twice(i) result(j)
integer, intent(in) :: i
integer :: j
j = 2*i
end function twice
pure function fourfold(i) result(j)
integer, intent(in) :: i
integer :: j
j = twice(twice(i))
end function fourfold
pure function half(x)
real, intent(in) :: x
real :: half
half = x/2.0
end function half
end module pure_functions
program main
use pure_functions
implicit none
integer :: pure
pure = 8
print *, twice(3), fourfold(3), half(3.0), pure
end program main
