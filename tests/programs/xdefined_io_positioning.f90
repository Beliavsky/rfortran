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
read(unit,'(t2,i1,tl1,i1)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
subroutine write_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,'(a,t2,i1,a)',iostat=iostat,iomsg=iomsg) '[ ]',dtv%x,']'
end subroutine
end module
program main
use runtime_defined_io
implicit none
type(point) :: p(2)
character(3) :: records(2)
character(20) :: fmt
p(1)%x=7
p(2)%x=9
fmt='(dt)'
write(records,fmt) p
print '(a)',records
p%x=0
fmt='(dt)'
read(records,fmt) p
print *,p%x,p%y
end program
