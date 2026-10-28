! Parent FINAL runs before its allocated components are finalized and released.
module owned_final_demo
implicit none
type :: resource
   integer :: value = 7
contains
   final :: finish_scalar, finish_vector
end type
type :: container
   type(resource), allocatable :: item
   type(resource), allocatable :: items(:)
contains
   final :: finish_container
end type
contains
subroutine finish_scalar(x)
   type(resource), intent(inout) :: x
   print *, 'scalar', x%value
end subroutine
subroutine finish_vector(x)
   type(resource), intent(inout) :: x(:)
   print *, 'vector', size(x), sum(x%value)
end subroutine
subroutine finish_container(x)
   type(container), intent(inout) :: x
   print *, 'parent', allocated(x%item), allocated(x%items)
end subroutine
end module

program demo
use owned_final_demo
implicit none
block
   type(container) :: x
   allocate(x%item, x%items(2))
   x%item%value = 3
   x%items%value = [4,5]
end block
end program
