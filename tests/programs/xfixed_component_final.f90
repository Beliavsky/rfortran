module fixed_component_final_demo
implicit none
type :: resource
   integer :: value = 7
contains
   final :: finish_resources
end type
type :: container
   type(resource) :: items(-1:1)
contains
   final :: finish_container
end type
contains
subroutine finish_resources(x)
type(resource), intent(inout) :: x(:)
print *, 'items', size(x), sum(x%value)
end subroutine
subroutine finish_container(x)
type(container), intent(inout) :: x
print *, 'parent', x%items%value
end subroutine
end module

program demo
use fixed_component_final_demo
implicit none
block
   type(container) :: x
   x%items%value = [2, 3, 4]
end block
end program
