module pointer_shapes
implicit none
type base
integer::value=1
contains
procedure::get=>base_get
procedure::add=>base_add
end type
type,extends(base)::child
integer::extra=10
contains
procedure::get=>child_get
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%value
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=self%value+self%extra
end function
subroutine base_add(self,n)
class(base),intent(inout)::self
integer,intent(in)::n
self%value=self%value+n
end subroutine
end module
module entries
use pointer_shapes
implicit none
type entry
class(base),pointer::object=>null()
end type
contains
subroutine show(x)
type(entry),intent(in)::x
print *,associated(x%object),x%object%get()
call x%object%add(2)
end subroutine
subroutine rebind(x,y)
type(entry),intent(inout)::x
type(child),target,intent(inout)::y
x%object=>y
end subroutine
subroutine clear(x)
type(entry),intent(out)::x
print *,associated(x%object)
end subroutine
end module
program p
use entries
implicit none
type(entry)::box
type(base),target::a
type(child),target::b
box%object=>a
call show(box)
print *,a%value
call rebind(box,b)
call show(box)
print *,associated(box%object,b),b%value
call clear(box)
print *,associated(box%object),a%value,b%value
end
