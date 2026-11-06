implicit none
integer :: i,x(9000),y(9000)
do i=1,9000
   x(i)=i
end do
do concurrent(i=1:9000) shared(x,y) default(none)
   call transform(x(i),y(i))
end do
if(any(y/=2*x+1)) stop 1
print *,y(1),y(9000)
contains
pure subroutine transform(x,y)
integer,intent(in)::x
integer,intent(out)::y
call twice(x,y)
call increment(y)
end subroutine
simple subroutine twice(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=2*x
end subroutine
pure subroutine increment(y)
integer,intent(inout)::y
y=y+1
end subroutine
end
