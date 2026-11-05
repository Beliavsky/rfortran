implicit none
integer :: i, n
real, allocatable :: totals(:,:)
n=2
allocate(totals(-1:0,3:n+2))
totals=7.0
do concurrent(i=1:10000) reduce(+:totals)
   totals=totals+1.0
end do
if (any(totals/=10007.0)) stop 1
print *, totals
end
