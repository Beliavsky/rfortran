module m
implicit none
type date
integer::day
end type
contains
pure function parse(s) result(x)
character(len=*),intent(in)::s
type(date)::x
character(len=len(s))::t
x=date(0)
t=adjustl(s)
if(len_trim(t)/=10) return
if(t(5:5)/='-'.or.t(8:8)/='-') return
x=date(1)
end function
end module
program main
use m
implicit none
type(date)::x
character(len=20)::text
text='2026-10-25'
x=parse(trim(text))
print *,x%day
end
