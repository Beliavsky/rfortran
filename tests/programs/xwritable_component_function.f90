module writable_component_function_mod
implicit none
type :: table
   real, allocatable :: values(:,:)
contains
   procedure :: positive
   procedure :: inspect
end type
contains
function positive(self) result(mask)
class(table), intent(inout) :: self
logical, allocatable :: mask(:,:)
self%values = self%values + 10.0
mask = self%values > 0.0
end function
subroutine inspect(self)
class(table), intent(inout) :: self
logical, allocatable :: mask(:,:)
mask = self%positive()
print *, shape(mask), count(mask)
end subroutine
end module

program main
use writable_component_function_mod
implicit none
type(table) :: x
integer :: n
read *, n
allocate(x%values(-1:0,4:n+3))
x%values = reshape([1.,-1.,2.,-2.,3.,-3.],[2,n])
call x%inspect()
print *, lbound(x%values), ubound(x%values), sum(x%values)
end program
