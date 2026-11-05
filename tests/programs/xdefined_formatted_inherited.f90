module inherited_output
implicit none
type :: base
   integer :: x
contains
   procedure :: emit => write_base
   procedure :: other => write_base
   generic :: write(formatted) => emit
end type
type, extends(base) :: child
   integer :: y
contains
   procedure :: emit => write_child
end type
type, extends(base) :: alias_child
contains
   procedure :: other => write_alias
end type
type, extends(child) :: grandchild
   integer :: z
end type
contains
subroutine write_base(dtv,unit,iotype,v_list,iostat,iomsg)
class(base),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
write(unit,'("base",i0)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
subroutine write_child(dtv,unit,iotype,v_list,iostat,iomsg)
class(child),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
write(unit,'("child",i0,":",i0)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
subroutine write_alias(dtv,unit,iotype,v_list,iostat,iomsg)
class(alias_child),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
write(unit,'("alias",i0)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
end module
program main
use inherited_output
implicit none
type(base) :: a
type(child) :: b
type(alias_child) :: c
type(grandchild) :: d
a%x=1
b%x=2
b%y=20
c%x=3
d%x=4
d%y=40
d%z=400
print *,a,b,c,d
write(*,'(4(dt,1x))') a,b,c,d
end program
