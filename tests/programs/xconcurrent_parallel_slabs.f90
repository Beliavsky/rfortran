! Run with --parallel --parallel-report and RFORT_NUM_THREADS=4.
implicit none
integer :: i, j, a(3,9001)
integer(kind=8) :: s
s = 7
do concurrent(i=1:9001) local(j) reduce(+:s)
   do j=1,3
      a(j,i) = 10*i+j
   end do
   a(1,i) = a(2,i)+1
   s = s + sum(a(:,i))
end do
do i=1,9001
   if (a(1,i)/=10*i+3) stop 1
   if (a(2,i)/=10*i+2) stop 2
   if (a(3,i)/=10*i+3) stop 3
end do
if (s/=7_8+sum(a)) stop 4
print *, "PASS"
end
