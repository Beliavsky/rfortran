module runtime_defined_io
implicit none
type :: point
   integer :: x,y
contains
   procedure :: read_point, write_point
   generic :: read(formatted) => read_point
   generic :: write(formatted) => write_point
end type
contains
subroutine read_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(inout) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
character(40) :: fmt
write(fmt,'("(2i",i0,")")') v_list(1)
read(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
subroutine write_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
end module
program main
use runtime_defined_io
implicit none
type(point) :: values(2)
character(6) :: records(2)
character(20) :: fmt
integer :: status
records = ['  1  2', '  3  4']
fmt = '(dt(3))'
read(records,fmt,iostat=status) values
if (status /= 0) stop 1
print *, values(1)%x,values(1)%y,values(2)%x,values(2)%y
end program
