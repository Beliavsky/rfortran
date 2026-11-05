module defined_input
implicit none
type :: point
   integer :: x,y
contains
   procedure :: read_point
   generic :: read(formatted) => read_point
end type
contains
subroutine read_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(inout) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x
if (iostat /= 0) return
read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%y
end subroutine
end module
program main
use defined_input
implicit none
type(point) :: a,b
integer :: u,status
character(100) :: message
open(newunit=u,status='scratch')
write(u,'(a)') '  7  9 99'
write(u,'(a)') ' 11 13 99'
rewind(u)
read(u,"(dt)") a
message='unchanged'
read(u,'(dt)',iostat=status,iomsg=message) b
print *,a%x,a%y,b%x,b%y,status,trim(message)
close(u)
end program
