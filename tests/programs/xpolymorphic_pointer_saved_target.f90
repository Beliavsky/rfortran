module persistent_owners
implicit none
type base
 integer :: value=1
contains
 procedure :: get
end type
type,extends(base)::child
 integer :: extra=10
contains
 procedure :: get=>child_get
end type
type entry
 class(base),pointer::object=>null()
end type
contains
integer function get(self) result(n)
class(base),intent(in)::self
n=self%value
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=self%value+self%extra
end function
subroutine attach(box)
type(entry),intent(inout)::box
type(child),target,save::target
box%object=>target
end subroutine
end module
program p
use persistent_owners
implicit none
type(entry)::a,b,c
call attach(a)
b=a
call attach(c)
print *,a%object%get(),associated(a%object,c%object)
a%object%value=8
print *,b%object%get(),c%object%get()
nullify(a%object)
print *,associated(a%object),b%object%get()
end
