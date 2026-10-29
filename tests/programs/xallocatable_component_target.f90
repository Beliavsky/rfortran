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
type(container),target::box
type(reference)::p,q
call create(box%item,p)
print *,box%item%value,p%object%value
call create(box%item,q)
p%object%value=9
print *,box%item%value,q%object%value,associated(p%object,q%object)
nullify(p%object,q%object)
deallocate(box%item)
end program
