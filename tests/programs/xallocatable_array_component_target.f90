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
type(container),target::boxes(3)
type(reference)::p,q
integer::i
i=1
call create(boxes(i)%item,p)
i=3
call create(boxes(i)%item,q)
p%object%value=7
q%object%value=9
print *,boxes(1)%item%value,boxes(3)%item%value
call move_alloc(boxes(1)%item,boxes(2)%item)
p%object%value=8
print *,allocated(boxes(1)%item),boxes(2)%item%value,associated(p%object,boxes(2)%item)
nullify(p%object,q%object)
deallocate(boxes(2)%item,boxes(3)%item)
end program
