! Fixed-size array reduction using a runtime-sized, read-only input array.
implicit none
integer :: i,n
real :: totals(-1:0)
real,allocatable :: values(:,:)
n=10000
allocate(values(2,n))
values(1,:)=1.0
values(2,:)=2.0
totals=7.0
do concurrent(i=1:n) reduce(+:totals)
   totals=totals+values(:,i)
end do
if(any(totals/=[10007.0,20007.0])) stop 1
print *,totals
end
