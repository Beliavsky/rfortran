module pointer_factories
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
type(child), target, private :: saved_target
contains
integer function base_get(self) result(n)
class(base), intent(in) :: self
n = self%value
end function
integer function child_get(self) result(n)
class(child), intent(in) :: self
n = self%value + self%extra
end function
function make_entry(attach) result(x)
logical, intent(in) :: attach
type(entry) :: x
if (.not. attach) return
x%object => saved_target
end function
end module

program p
use pointer_factories
implicit none
type(entry) :: first, second
first = make_entry(.true.)
second = make_entry(.true.)
first%object%value = 4
print *, first%object%get(), second%object%get()
print *, associated(first%object, second%object)
first = make_entry(.false.)
print *, associated(first%object), second%object%get()
end
