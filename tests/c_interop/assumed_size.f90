! COUNT controls the algorithm; it is not a hidden array descriptor.
function total(x, count) result(r) bind(c)
use iso_c_binding, only: c_int, c_double
implicit none
integer(c_int), value, intent(in) :: count
real(c_double), intent(in) :: x(*)
real(c_double) :: r
integer :: j
r = 0.0_c_double
do j = 1, count
   r = r + x(j)
end do
end function total

subroutine fill(x, count) bind(c)
use iso_c_binding, only: c_int, c_double
implicit none
integer(c_int), value, intent(in) :: count
real(c_double), intent(out) :: x(*)
integer :: j
do j = 1, count
   x(j) = 0.5_c_double*j
end do
end subroutine fill
