module defined_output_example
implicit none
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
write(unit,'("[",i0,"]")',iostat=iostat,iomsg=iomsg) dtv%x
if(iostat/=0) return
write(unit,'(a)',iostat=iostat,iomsg=iomsg) '!'

end subroutine
end module
program main
use defined_output_example
implicit none
type(point) :: a,b
integer :: u,status
character(100) :: line,message
a%x=7
b%x=9
open(newunit=u,status='scratch')
message='unchanged'
write(u,*,iostat=status,iomsg=message) 'begin',a,b,'end'
print *,status,trim(message)
write(u,'(dt)') a
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
end program
