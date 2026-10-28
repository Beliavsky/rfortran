module saved_owner_examples
implicit none
type base
 integer :: value=1
end type
type,extends(base)::child
 integer :: extra=10
end type
type entry
 class(base),pointer::object=>null()
end type
type(entry),private::current
type(base),target,private::first
type(child),target,private::second
contains
subroutine attach(choose)
logical,intent(in)::choose
if(choose)then
 current%object=>first
else
 current%object=>second
end if
end subroutine
subroutine add(n)
integer,intent(in)::n
current%object%value=current%object%value+n
end subroutine
subroutine snapshot(x)
type(entry),intent(out)::x
x=current
end subroutine
subroutine clear()
nullify(current%object)
end subroutine
logical function connected() result(tf)
tf=associated(current%object)
end function
subroutine saved(install,n)
logical,intent(in)::install
integer,intent(in)::n
type(entry),save::cache
if(install)cache%object=>second
cache%object%value=cache%object%value+n
print *,cache%object%value
end subroutine
end module
program p
use saved_owner_examples
implicit none
type(entry)::copy
print *,connected()
call attach(.false.)
call add(4)
call snapshot(copy)
print *,connected(),copy%object%value
call clear()
print *,connected(),copy%object%value
call saved(.true.,2)
call saved(.false.,3)
print *,copy%object%value
end
