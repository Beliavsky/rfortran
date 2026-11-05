! Rank-three runtime storage, nondefault bounds, and a scalar reduction.
implicit none
integer :: i,j,k,total
real,allocatable :: a(:,:,:)
allocate(a(-2:17,0:19,3:22))
total=0
do concurrent(k=3:22,j=0:19,i=-2:17) reduce(+:total)
   a(i,j,k)=real(i+j+k)
   total=total+1
end do
if(total/=8000) stop 1
if(a(-2,0,3)/=1.0 .or. a(17,19,22)/=58.0) stop 2
print *,total,a(-2,0,3),a(17,19,22)
end
