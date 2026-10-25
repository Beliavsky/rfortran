! Derived-type result and optional CHARACTER arguments with a generic interface.
module boxes
implicit none
type box
integer :: n=1
contains
procedure :: copy
end type
interface copy_box
module procedure copy
end interface
contains
function copy(self,axis,how) result(out)
class(box),intent(in)::self
character(len=*),intent(in),optional::axis,how
type(box)::out
out%n=self%n
if(present(axis)) out%n=out%n+len_trim(axis)
if(present(how)) out%n=out%n+len_trim(how)
end function
end module
program main
use boxes
implicit none
type(box)::x,y
y=x%copy()
print *,y%n
y=x%copy(how='all')
print *,y%n
y=x%copy('rows','any')
print *,y%n
y=copy_box(how="",self=x,axis="a"//"bc")
print *,y%n
end program
