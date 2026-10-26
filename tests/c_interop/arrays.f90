! Explicit-shape C array example. Storage is in column-major order.
subroutine scale_matrix(rows, cols, x, factor) bind(c, name="ScaleMatrix")
use iso_c_binding, only: c_int, c_float
implicit none
integer(c_int), value, intent(in) :: rows, cols
real(c_float), intent(inout) :: x(rows, cols)
real(c_float), value, intent(in) :: factor
integer :: i, j
do j = 1, cols
   do i = 1, rows
      x(i,j) = factor*x(i,j)
   end do
end do
end subroutine scale_matrix
