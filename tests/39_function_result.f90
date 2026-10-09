module result_functions
implicit none
contains
function twice(i) result(j)
integer, intent(in) :: i
integer :: j
j = 2*i
end function twice
function positive(i) result(j)
integer, intent(in) :: i
integer :: j
j = 0
if (i < 0) return
j = i
end function positive
function half(x) result(y)
real, intent(in) :: x
real :: y
y = x / 2.0
end function half
end module result_functions
program main
use result_functions
implicit none
integer :: result
result = 8
print *, twice(3), positive(-2), positive(5), half(3.0), result
end program main
