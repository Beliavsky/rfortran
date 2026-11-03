module labels_mod
implicit none
contains
subroutine copy_labels(labels, override)
character(len=*), intent(in) :: labels(:)
character(len=*), intent(in), optional :: override(:)
character(len=:), allocatable :: heading, selected(:)
heading = 'labels'
if (present(override)) then
   selected = override
else
   selected = labels
end if
print '(a,2(1x,i0))', heading, size(selected), len(selected)
print '(*(a,:,","))', selected
end subroutine
end module

! Saved-object preparation requests constant folding before array lowering.
module state_mod
implicit none
type wrapper
   real :: value
end type
contains
subroutine activate()
type(wrapper), allocatable, target :: object
type(wrapper), save :: saved = wrapper(0.)
allocate(object)
object%value = saved%value
deallocate(object)
end subroutine
end module

program main
use labels_mod
use state_mod
implicit none
call activate()
call copy_labels(['one', 'two'])
call copy_labels(['one', 'two'], ['four', 'five', 'nine'])
end program
