module binary_points
implicit none
type :: point
   integer :: x=0, y=0
contains
   procedure :: write_binary
   procedure :: read_binary
   generic :: write(unformatted) => write_binary
   generic :: read(unformatted) => read_binary
end type
contains
subroutine write_binary(dtv,unit,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
! A deliberately different layout from default component-wise output.
write(unit,iostat=iostat,iomsg=iomsg) dtv%y+100,dtv%x
end subroutine
subroutine read_binary(dtv,unit,iostat,iomsg)
class(point),intent(inout) :: dtv
integer,intent(in) :: unit
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
integer :: encoded
read(unit,iostat=iostat,iomsg=iomsg) encoded,dtv%x
if (iostat==0) dtv%y=encoded-100
end subroutine
end module
program main
use binary_points
implicit none
type(point) :: p, q
integer :: unit,ios,a,b
character(80) :: msg
p=point(7,13)
open(newunit=unit,file='defined-unformatted.bin',access='stream', &
     form='unformatted',status='replace')
msg='unchanged'
write(unit,iostat=ios,iomsg=msg) p
print *,ios,trim(msg)
rewind(unit)
read(unit) a,b
print *,a,b
rewind(unit)
read(unit,iostat=ios,iomsg=msg) q
print *,ios,q%x,q%y,trim(msg)
close(unit,status='delete')
end program
