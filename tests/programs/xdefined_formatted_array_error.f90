module defined_output_example
implicit none
integer :: calls=0
type :: point
   integer :: x
contains
   procedure :: write_point
   generic :: write(formatted) => write_point
end type
contains
subroutine write_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
calls=calls+1
if(dtv%x==2) then
 iostat=17
 iomsg='element failed'
 return
end if
write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x

end subroutine
end module
program main
use defined_output_example
implicit none
type(point) :: a,b,points(3)
integer :: u,status
character(100) :: line,message
a%x=7
b%x=9
points(1)=point(1)
points(2)=point(2)
points(3)=point(3)
open(newunit=u,status='scratch')
write(u,*,iostat=status,iomsg=message) points,b
print *,status,trim(message),calls
close(u)

end program
