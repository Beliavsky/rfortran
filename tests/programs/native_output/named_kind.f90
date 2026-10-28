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
module owner_entries
use pointer_shapes
implicit none
integer,parameter::ik=selected_int_kind(12),dp=kind(1.0d0)
type entry
class(base),pointer::object=>null()
contains
procedure::get=>entry_get
procedure::add=>entry_add
procedure::rebind=>entry_rebind
procedure::set_value=>entry_set_value
procedure::clear=>entry_clear
procedure::show=>entry_show
procedure::put_integer,put_real
generic::put=>put_integer,put_real
generic::set=>rebind,set_value
generic::value=>get
end type
type,extends(entry)::special_entry
integer::bonus=100
contains
procedure::get=>special_get
end type
interface fetch
module procedure fetch_entry,fetch_integer
end interface
contains
integer function entry_get(self) result(n)
class(entry),intent(in)::self
n=-1
if(associated(self%object)) n=self%object%get()
end function
integer function special_get(self) result(n)
class(special_entry),intent(in)::self
n=self%entry%get()+self%bonus
end function
subroutine entry_add(self,n)
class(entry),intent(in)::self
integer,intent(in)::n
call self%object%add(n)
end subroutine
subroutine entry_rebind(self,target)
class(entry),intent(inout)::self
type(child),target,intent(inout)::target
self%object=>target
end subroutine
subroutine entry_set_value(self,n)
class(entry),intent(in)::self
integer,intent(in)::n
self%object%value=n
end subroutine
subroutine entry_clear(self)
class(entry),intent(out)::self
end subroutine
subroutine entry_show(self)
class(entry),intent(in)::self
print *,self%get()
end subroutine
integer function fetch_entry(x) result(n)
class(entry),intent(in)::x
n=x%get()
end function
integer function fetch_integer(x) result(n)
integer,intent(in)::x
n=2*x
end function
subroutine put_integer(self,n)
class(entry),intent(in)::self
integer(kind=ik),intent(in)::n
self%object%value=int(n)
end subroutine
subroutine put_real(self,n)
class(entry),intent(in)::self
real(kind=dp),intent(in)::n
self%object%value=int(n)
end subroutine
end module
program p
use owner_entries
implicit none
type(entry)::x
type(child),target::a
call x%rebind(a)
call x%put(7_ik)
print *,x%get()
call x%put(n=9.0_dp)
print *,x%get()
end
