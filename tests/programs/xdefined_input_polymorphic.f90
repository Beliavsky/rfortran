module polymorphic_input
implicit none
type :: base
 integer :: x=0
contains
 procedure :: read_value => read_base
 generic :: read(formatted) => read_value
end type
type,extends(base) :: child
 integer :: y=0
contains
 procedure :: read_value => read_child
end type
type,extends(base) :: inherited
end type
contains
subroutine read_base(dtv,unit,iotype,v_list,iostat,iomsg)
class(base),intent(inout) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
read(unit,'(i3)',iostat=iostat,iomsg=iomsg) dtv%x
end subroutine
subroutine read_child(dtv,unit,iotype,v_list,iostat,iomsg)
class(child),intent(inout) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
subroutine fill(value,text)
class(base),intent(inout) :: value
character(*),intent(in) :: text
read(text,'(dt)') value
end subroutine
end module
program main
use polymorphic_input
implicit none
type(base) :: a
type(child) :: b
type(inherited) :: c
call fill(a,'  1')
call fill(b,'  2  3')
call fill(c,'  4')
print *,a%x,b%x,b%y,c%x
end program
