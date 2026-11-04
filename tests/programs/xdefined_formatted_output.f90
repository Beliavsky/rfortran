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
write(unit,'("[",i0,"]")') dtv%x
write(unit,'(a)') '!'
iostat=0
end subroutine
end module
program main
use defined_output_example
implicit none
type(point) :: a,b
a%x=7
b%x=9
print *, 'begin',a,b,'end'
write(*,'(dt)') a
print *, 'done'
end program
