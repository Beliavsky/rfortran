implicit none
integer :: n,i,k
real, allocatable :: x(:), y(:)
real :: total
n=10000
do k=1,3
   allocate(x(n),y(n))
   x=4.0
   total=7.0
   do concurrent(i=1:n) shared(x,y) reduce(+:total) default(none)
      y(i)=sqrt(x(i))
      total=total+y(i)
   end do
   print *,total,y(1),y(n)
   deallocate(x,y)
   n=n+1
end do
end
