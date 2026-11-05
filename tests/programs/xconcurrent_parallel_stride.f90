! Runtime negative stride, fixed matrix column, offset, and scalar reduction.
implicit none
integer :: i,n,stride,slot,total
integer,allocatable :: a(:,:)
n=18000
stride=-2
slot=2
allocate(a(0:n+1,2))
a=-1
total=0
do concurrent(i=n:1:stride) reduce(+:total)
   a(i+1,slot)=i
   total=total+a(i+1,slot)
end do
if(total/=81009000) stop 1
if(a(3,slot)/=2 .or. a(n+1,slot)/=n) stop 2
if(a(2,slot)/=-1) stop 3
print *,total,a(3,slot),a(n+1,slot)
end
