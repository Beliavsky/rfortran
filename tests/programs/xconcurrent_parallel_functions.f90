implicit none
integer :: i,n
real,allocatable :: x(:),y(:)
n=9000
allocate(x(n),y(n))
x=3.0
do concurrent(i=1:n)
   y(i)=transform(x(i))
end do
if(any(y/=10.0)) stop 1
print *,y(1),y(n)
contains
pure real function transform(t) result(r)
real,intent(in)::t
r=twice(t)+4.0
end function
simple real function twice(t)
real,intent(in)::t
twice=2.0*t
end function
end
