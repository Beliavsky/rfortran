module derived_math
use base_math, only: multiply, factor
implicit none
contains
pure function calculate(i) result(j)
integer, intent(in) :: i
integer :: j
j = multiply(i)+factor
end function calculate
end module derived_math
