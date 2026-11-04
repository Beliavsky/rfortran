module defined_output_example
implicit none
type :: point
   integer :: x
contains
   procedure :: write_point
   generic :: write(formatted) => write_point
end type
type :: container
 type(point) :: item
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
type(point) :: a,b
type(container) :: box
box%item%x=11
a%x=7
b%x=9
print *, box%item, point(12), point(x=13)
write(*,'(dt)') box%item

end program
