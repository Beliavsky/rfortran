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
program p
use pointer_shapes
implicit none
class(base),pointer::p1,p2
type(base),target::a
type(child),target::b,c
p1=>a
print *,p1%get()
p1=>b
p2=>p1
call p1%add(2)
print *,b%value,p2%get(),associated(p1,p2)
p1=>c
call p1%add(4)
print *,p1%get(),p2%get()
nullify(p1)
print *,associated(p1),associated(p2,b)
end
