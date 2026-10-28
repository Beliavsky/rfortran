module m
implicit none
integer :: calls=0
type :: base
integer :: x=1,y=2
end type
type, extends(base) :: child
integer :: z=3
end type
type, extends(child) :: grandchild
integer :: w=4
end type
contains
function make(n) result(r)
integer,intent(in)::n
type(base)::r
calls=calls+1
r=base(n,n+1)
end function
integer function total(a) result(n)
type(child),intent(in)::a
n=a%x+a%y+a%z
end function
end module
use m
implicit none
integer::i
i=0
do while(total(child(make(i),0))<6)
i=i+1
if(i==1) cycle
end do
print *,i,calls
end
