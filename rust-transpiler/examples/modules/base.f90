module base_math
implicit none
integer, parameter :: factor = 2
contains
pure function multiply(i) result(j)
integer, intent(in) :: i
integer :: j
j = factor*i
end function multiply
end module base_math
