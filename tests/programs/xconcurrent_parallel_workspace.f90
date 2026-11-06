implicit none
integer :: i,n
real,allocatable :: x(:,:),y(:)
n=4
allocate(x(n,9000),y(9000))
x=2.0
do concurrent(i=1:9000)
   y(i)=energy(x(:,i))
end do
if(any(y/=16.0)) stop 1
print *,y(1),y(9000)
contains
pure real function energy(a)
real,intent(in)::a(:)
real::work(size(a))
work=a*a
energy=sum(work)
end function
end
