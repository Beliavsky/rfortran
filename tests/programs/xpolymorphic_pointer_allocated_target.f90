module allocated_target_examples
implicit none
type base
   integer :: value = 1
contains
   procedure :: get => base_get
end type
type, extends(base) :: child
   integer :: extra = 10
contains
   procedure :: get => child_get
end type
type entry
   class(base), pointer :: object => null()
end type
type(child), allocatable, target :: target
contains
integer function base_get(self) result(n)
class(base), intent(in) :: self
n = self%value
end function
integer function child_get(self) result(n)
class(child), intent(in) :: self
n = self%value + self%extra
end function
function make_entry() result(x)
type(entry) :: x
if (.not. allocated(target)) allocate(target)
x%object => target
end function
end module

program p
use allocated_target_examples
implicit none
type(entry) :: first, second
first = make_entry()
second = first
first%object%value = 4
print *, second%object%get(), associated(first%object, second%object)
nullify(first%object)
print *, allocated(target), associated(first%object), second%object%get()
nullify(second%object)
deallocate(target)
print *, allocated(target)
end
