module component_forward_mod
implicit none
type :: table
   real, allocatable :: values(:,:)
contains
   procedure :: positive
   procedure :: inspect
end type
contains
function positive(self) result(mask)
class(table), intent(in) :: self
logical, allocatable :: mask(:,:)
if (.not. allocated(self%values)) error stop "values not allocated"
mask = self%values > 0.0
end function
subroutine inspect(self)
class(table), intent(in) :: self
logical, allocatable :: mask(:,:)
mask = self%positive()
print *, shape(mask), count(mask)
end subroutine
end module
program main
use component_forward_mod
implicit none
type(table) :: x
allocate(x%values(2,3))
x%values = reshape([1.,-1.,2.,-2.,3.,-3.],[2,3])
call x%inspect()
end program
