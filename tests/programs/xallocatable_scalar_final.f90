module allocatable_scalar_final_demo
implicit none
type :: resource
   integer :: value = 7
contains
   final :: finish
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
subroutine release(x)
type(resource), allocatable, intent(inout) :: x
deallocate(x)
end subroutine
end module

program demo
use allocatable_scalar_final_demo
implicit none
type(resource), allocatable :: x
allocate(x)
x%value = 3
call reset(x)
print *, allocated(x), x%value
call release(x)
print *, allocated(x)
end program
