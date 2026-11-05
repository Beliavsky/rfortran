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
read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
end module
program main
use defined_input
implicit none
integer :: u,status
character(100) :: message
integer :: n
type(point) :: a(2)
real :: r
open(newunit=u,status='scratch')
write(u,'(a)') ' 10  1  2  3  4  5.5'
rewind(u)
read(u,"(i3,2dt,f5.1)") n,a,r
print *,n,a%x,a%y,r
close(u)
end program
