! IEEE_ARITHMETIC subset supported by rfort.
program test_ieee_arithmetic
use, intrinsic :: iso_fortran_env, only: real64
use, intrinsic :: ieee_arithmetic, only: ieee_value, ieee_quiet_nan, &
   ieee_positive_inf, ieee_is_nan, ieee_is_finite, ieee_is_normal, ieee_unordered
implicit none
real(real64) :: x(3)
x = [1.0_real64, ieee_value(0.0_real64,ieee_quiet_nan), &
     ieee_value(0.0_real64,ieee_positive_inf)]
print *, ieee_is_nan(x)
print *, ieee_is_finite(x)
print *, ieee_is_normal(x)
print *, ieee_unordered(x,0.0_real64)
where (ieee_is_nan(x)) x=0.0_real64
print *, ieee_is_nan(x)
end program test_ieee_arithmetic
