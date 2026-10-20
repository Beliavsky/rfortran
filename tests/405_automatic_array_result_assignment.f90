program test_array_result
implicit none
real, allocatable :: x(:)
x = make_array(3)
print *, x
contains
function make_array(n) result(y)
integer, intent(in) :: n
real :: y(n)
integer :: i
do i=1,n
   y(i) = 10.0 * i
end do
end function make_array
end program test_array_result
