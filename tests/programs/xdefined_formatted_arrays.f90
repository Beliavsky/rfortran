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
write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
end module
program main
use defined_output_example
implicit none
type(point) :: a,b,points(0:1,3:4)
a%x=7
b%x=9
points(0,3)=point(1)
points(1,3)=point(2)
points(0,4)=point(3)
points(1,4)=point(4)
print *, points

end program
