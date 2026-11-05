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
read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
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
integer :: n
real :: r
character(40) :: fmt
character(40) :: input,output
fmt='(i3,2dt,f5.1)'
input=' 10  1  2  3  4  5.5'
read(input,fmt) n,values,r
write(output,fmt) n,values,r
print '(a)',trim(output)
write(*,fmt) n,values,r
end program
