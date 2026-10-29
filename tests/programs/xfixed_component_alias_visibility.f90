module fixed_component_aliases
implicit none
type :: box
   integer, allocatable :: owned(:)
   integer :: counts(-1:1) = 0
   character(len=7) :: name = 'initial'
end type
type(box), target :: boxes(0:2)
contains
subroutine update(x)
type(box), target, intent(inout) :: x
x%counts(-1) = 7
x%name = 'updated'
print *, boxes(1)%counts(-1), boxes(1)%name
boxes(1)%counts(1) = 9
boxes(1)%name = 'reverse'
print *, x%counts(1), x%name
allocate(x%owned(1))
x%owned = 11
end subroutine
end module
program demo
use fixed_component_aliases
implicit none
call update(boxes(1))
print *, boxes(1)%counts(-1), boxes(1)%counts(0), boxes(1)%counts(1), boxes(1)%name, boxes(1)%owned(1)
end
