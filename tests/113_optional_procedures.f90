program optional_procedures
implicit none
character(5)::label='start',output
real::x
x=5.0
print *,scale(x),scale(x,2.0)
call show(x)
call show(x,'tag:')
call show(x,label)
call show(x,'a'//'bc')
call show(x,'')
call forward(x)
call forward(x,3.0)
call change(output)
print *,'[',output,']'
call change(output,'done')
print *,'[',output,']'
print *,swapped(10),swapped(10,2)
contains
real function scale(x,factor)
real,intent(in)::x
real,intent(in),optional::factor
if (present(factor)) then
scale=x*factor
else
scale=x
end if
end function scale
subroutine show(x,text)
real,intent(in)::x
character(*),intent(in),optional::text
if (present(text)) then
print *,'[',text,']',len(text),x
else
print *,x
end if
end subroutine show
subroutine forward(x,factor)
real,intent(in)::x
real,intent(in),optional::factor
print *,scale(x,factor)
end subroutine forward
subroutine change(dest,text)
character(*),intent(out)::dest
character(*),intent(in),optional::text
if (.not.present(text)) then
dest='none'
else
dest=text
end if
end subroutine change
integer function pair(x,a,b)
integer,intent(in)::x
integer,intent(in),optional::a,b
pair=x
if(present(a)) pair=pair+a
if(present(b)) pair=pair+10*b
end function pair
integer function swapped(x,a,b)
integer,intent(in)::x
integer,intent(in),optional::a,b
swapped=pair(x,b,a)
end function swapped
end program optional_procedures
