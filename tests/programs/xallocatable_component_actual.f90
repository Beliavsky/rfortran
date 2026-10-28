module allocatable_component_demo
implicit none
type :: resource
   integer :: value = 7
contains
   final :: finish
end type
type :: owner
   type(resource), allocatable :: item
end type
contains
subroutine finish(x)
type(resource), intent(inout) :: x
print *, 'final', x%value
end subroutine
subroutine reset(x)
type(resource), allocatable, intent(out) :: x
print *, 'entry', allocated(x)
allocate(x)
x%value = 9
end subroutine
subroutine show(x)
type(resource), allocatable, intent(in) :: x
print *, allocated(x)
if (allocated(x)) print *, x%value
end subroutine
end module

program demo
use allocatable_component_demo
implicit none
type(owner) :: box
call reset(box%item)
call show(box%item)
call reset(box%item)
deallocate(box%item)
end program
