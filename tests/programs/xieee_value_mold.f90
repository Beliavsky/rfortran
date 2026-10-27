! IEEE_VALUE uses X for type and kind, not its stored value.
program test_ieee_value_mold
use, intrinsic :: ieee_arithmetic, only: ieee_value, ieee_quiet_nan, ieee_is_nan
implicit none
real :: x
x=ieee_value(x,ieee_quiet_nan)
print *,ieee_is_nan(x)
end program
