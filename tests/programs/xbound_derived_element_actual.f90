module m
implicit none
type date
integer :: day
end type
type frame
integer :: offset=10
contains
procedure :: row_pos => locate
procedure :: update => change
end type
contains
integer function locate(self,idx) result(pos)
class(frame),intent(in)::self
type(date),intent(in)::idx
pos=self%offset+idx%day
end function
subroutine change(self,idx)
class(frame),intent(in)::self
type(date),intent(inout)::idx
idx%day=self%offset+idx%day
end subroutine
subroutine select_rows(self,rows)
class(frame),intent(in)::self
type(date),intent(in),optional::rows(:)
integer::k
if(present(rows)) then
do k=1,size(rows)
print *,self%row_pos(rows(k))
end do
end if
end subroutine
end module
program main
use m
implicit none
type(frame)::df
type(date)::rows(2)
rows=[date(2),date(5)]
call select_rows(df,rows)
call df%update(rows(2))
print *,rows%day
end
