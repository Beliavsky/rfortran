module m
implicit none
type box
integer :: n=3
contains
procedure :: copy
end type
contains
pure function copy(self) result(out)
class(box),intent(in) :: self
type(box) :: out
out%n=twice(self%n)
contains
pure integer function twice(i) result(j)
integer,intent(in) :: i
j=2*i
end function
end function
end module
program main
use m
implicit none
type(box)::x,y
y=x%copy()
print *,y%n
end program
