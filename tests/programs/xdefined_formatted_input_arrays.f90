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
read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x
if (iostat /= 0) return
read(unit,*,iostat=iostat,iomsg=iomsg) dtv%y
end subroutine
end module
program main
use defined_input
implicit none
integer :: u,status
character(100) :: message
type(point) :: a(2,3)
open(newunit=u,status='scratch')
write(u,'(a)') '1 2 3 4 5 6 7 8 9 10 11 12'
rewind(u)
a%x=-1
a%y=-1
read(u,*) a(:,3:1:-1)
print *,a%x
print *,a%y
close(u)
end program
