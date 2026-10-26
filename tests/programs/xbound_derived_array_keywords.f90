module m
implicit none
type date
integer::day,month
end type
type frame
integer::offset=10
contains
procedure::select=>choose
procedure::adjust=>update
end type
contains
function choose(self,rows,extra) result(out)
class(frame),intent(in)::self
type(date),intent(in),optional::rows(:)
integer,intent(in),optional::extra
type(frame)::out
out%offset=self%offset
if(present(rows)) out%offset=out%offset+sum(rows%day)+sum(rows%month)
if(present(extra)) out%offset=out%offset+extra
end function
subroutine update(self,rows,extra)
class(frame),intent(in)::self
type(date),intent(inout)::rows(:)
integer,intent(in)::extra
rows%day=rows%day+self%offset+extra
end subroutine
end module
program main
use m
implicit none
type(frame)::df,out
type(date)::rows(2)
rows=[date(2,1),date(5,3)]
out=df%select(rows=rows,extra=4)
print *,out%offset
call df%adjust(extra=1,rows=rows)
print *,rows%day
out=df%select(extra=2)
print *,out%offset
end
