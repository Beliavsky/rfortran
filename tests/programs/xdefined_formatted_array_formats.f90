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
type(point) :: a,b,points(0:2,2)
integer :: i,j
integer, allocatable :: indices(:)
a%x=7
b%x=9
do j=1,2
 do i=0,2
  points(i,j)=point(10*j+i)
 end do
end do
allocate(indices(-1:1))
indices=[2,0,2]
write(*,'(*(dt,:,", "))') points(indices,2:1:-1)
write(*,'(2(dt,1x))') points([1,0],[2,1,2])
deallocate(indices)

end program
