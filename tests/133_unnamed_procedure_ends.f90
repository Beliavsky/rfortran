module m
contains
subroutine sub()
print *, f(10)
end subroutine

integer function f(i)
integer, intent(in) :: i
f = 2*i
end

end module

use m
implicit none
call sub()
end
