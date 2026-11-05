implicit none
integer :: i,m,n
real,allocatable :: s(:),x(:,:)
m=3
n=9000
allocate(s(m),x(m,n))
x(1,:)=1.0
x(2,:)=2.0
x(3,:)=3.0
s=7.0
do concurrent(i=1:n) reduce(+:s)
   s(1:m)=s(1:m)+x(1:m,i)
end do
if(any(s/=[9007.0,18007.0,27007.0])) stop 1
print *,s
end
