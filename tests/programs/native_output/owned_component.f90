module pointer_shapes
implicit none
type base
integer::value=1
real,allocatable::data(:)
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
module owners
use pointer_shapes
implicit none
type entry
class(base),pointer::object=>null()
end type
type(entry)::current
type(child),allocatable,target::target
end module
program p
use owners
implicit none
type(entry)::copy
allocate(target)
target%data=[1.0,2.0,3.0]
copy%object=>target
print *,sum(copy%object%data)
nullify(copy%object)
deallocate(target)
end
