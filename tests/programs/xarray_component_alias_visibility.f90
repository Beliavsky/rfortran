! The dummy and the module name must describe the same live allocation.
module array_component_alias_mod
implicit none
type :: item_type
   integer :: value=1
end type
type :: box_type
   type(item_type), allocatable :: item
end type
type(box_type), target :: boxes(3)
contains
subroutine update(x)
type(item_type), allocatable, target, intent(inout) :: x
allocate(x)
x%value=7
print *, allocated(boxes(2)%item), boxes(2)%item%value
boxes(2)%item%value=9
print *, x%value
deallocate(boxes(2)%item)
print *, allocated(x)
end subroutine
end module

program demo
use array_component_alias_mod
implicit none
call update(boxes(2)%item)
print *, allocated(boxes(2)%item)
end
