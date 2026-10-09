module pairs
implicit none
type :: pair
integer :: first = 1
integer :: second
contains
procedure :: get_first
procedure :: reset
end type pair
contains
pure integer function get_first(self)
class(pair), intent(in) :: self
get_first = self%first
end function get_first
subroutine reset(self)
class(pair), intent(out) :: self
self%second = 9
end subroutine reset
end module pairs
program components
use pairs
implicit none
type(pair) :: a, b
! An unread, unset component does not prevent reading another component.
print *, a%get_first()
a%first = 7
call a%reset()
print *, a%first, a%second
b = a
b%second = 12
print *, a%second, b%second
b = pair(b%second, b%first)
print *, b%first, b%second
print *, get_first(pair(5, 6))
end program components
