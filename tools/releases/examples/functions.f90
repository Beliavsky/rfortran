module arithmetic
implicit none
contains
pure integer function twice(n) result(value)
integer, intent(in) :: n
value = 2*n
end function twice
end module arithmetic

program functions
use arithmetic, only: twice
implicit none
print *, twice(21)
end program functions
