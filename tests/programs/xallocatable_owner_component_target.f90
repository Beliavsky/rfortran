module component_target_mod
implicit none
type :: resource
   integer :: value=1
end type
type :: container
   type(resource),allocatable :: item
end type
type :: reference
   class(resource),pointer :: object=>null()
end type
contains
subroutine create(x,p)
type(resource),allocatable,target,intent(inout)::x
type(reference),intent(out)::p
if(.not.allocated(x)) allocate(x)
x%value=x%value+2
p%object=>x
end subroutine
end module
program demo
use component_target_mod
implicit none
type(container), allocatable, target :: boxes(:), other(:)
type(reference) :: p
allocate(boxes(-1:1))
call create(boxes(0)%item,p)
p%object%value=7
print *,boxes(0)%item%value
call move_alloc(boxes,other)
p%object%value=9
print *,allocated(boxes),other(0)%item%value,associated(p%object,other(0)%item)
nullify(p%object)
deallocate(other)
end
