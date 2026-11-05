module complex_mixed_io
implicit none
type :: point
   integer :: x
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
read(unit,'(i3)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
subroutine write_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,'(i3)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
end module
program main
use complex_mixed_io
implicit none
integer,parameter :: dp=kind(0d0)
complex(dp) :: z,w
type(point) :: p
character(80) :: buffer,fmt
z=cmplx(1.25_dp,-2.5_dp,dp)
w=cmplx(3.5_dp,4.75_dp,dp)
p%x=7
fmt='(2f8.2,dt,2f8.2)'
write(buffer,fmt) z,p,w
print '(a)',trim(buffer)
z=0
w=0
p%x=0
read(buffer,fmt) z,p,w
print '(4f8.2,i3)',real(z),aimag(z),real(w),aimag(w),p%x
end program
