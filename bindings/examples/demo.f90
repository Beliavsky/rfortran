! This library is compiled by gfortran, not translated to Rust.
module binding_demo
   use iso_c_binding, only: c_double, c_float, c_int32_t, c_int64_t
   implicit none
contains
   function twice(x) result(y)
      real(c_double), intent(in) :: x
      real(c_double) :: y
      y = 2.0_c_double*x
   end function twice

   subroutine affine(x, a, b)
      real(c_double), intent(inout) :: x(:)
      real(c_double), intent(in) :: a, b
      x = a*x + b
   end subroutine affine

   subroutine add(x, y, z)
      real(c_double), intent(in) :: x(:), y(:)
      real(c_double), intent(out) :: z(:)
      z = x + y
   end subroutine add

   function total(x) result(value)
      real(c_double), intent(in) :: x(:)
      real(c_double) :: value
      value = sum(x)
   end function total

   subroutine classify(x, code)
      real(c_double), intent(in) :: x
      integer(c_int32_t), intent(out) :: code
      code = 0
      if (x > 0.0_c_double) code = 1
      if (x < 0.0_c_double) code = -1
   end subroutine classify

   subroutine increment(x)
      integer(c_int64_t), intent(inout) :: x
      x = x + 1_c_int64_t
   end subroutine increment

   function twice_single(x) result(y)
      real(c_float), intent(in) :: x
      real(c_float) :: y
      y = 2.0_c_float*x
   end function twice_single
end module binding_demo
