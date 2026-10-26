! Numeric assumed-shape C binding, including strided C arrays.
subroutine offset_array(x, delta) bind(c, name="OffsetArray")
use iso_c_binding, only: c_double
implicit none
real(c_double), intent(inout) :: x(:,:)
real(c_double), value, intent(in) :: delta
x = x + delta
end subroutine offset_array
