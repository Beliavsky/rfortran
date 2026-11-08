! Run with --parallel --parallel-report and RFORT_NUM_THREADS=4.
implicit none
integer :: i, j, total, a(9001)
integer(kind=8) :: s
s = 7
do concurrent(i=1:9001) local(j,total) reduce(+:s)
   j = 0
   total = 0
   do while (j<5)
      j = j + 1
      if (j==2) cycle
      if (j==4) exit
      total = total + j*i
   end do
   a(i) = total
   s = s + total
end do
do i=1,9001
   if (a(i)/=4*i) stop 1
end do
if (s/=7_8+2_8*9001*9002) stop 2
print *, "PASS"
end
