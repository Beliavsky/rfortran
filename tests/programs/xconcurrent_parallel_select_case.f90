! Run with --parallel --parallel-report and RFORT_NUM_THREADS=4.
implicit none
integer :: i, a(9001)
integer(kind=8) :: s
a = 0
s = 7
do concurrent(i=1:9001) reduce(+:s)
   select case (mod(i,4))
   case (0)
      cycle
   case (1,3)
      a(i) = i
   case default
      a(i) = 2*i
   end select
   s = s + a(i)
end do
do i=1,9001
   if (mod(i,4)==0) then
      if (a(i)/=0) stop 1
   else if (mod(i,4)==2) then
      if (a(i)/=2*i) stop 2
   else
      if (a(i)/=i) stop 3
   end if
end do
if (s/=7_8+sum(a)) stop 4
print *, "PASS"
end
