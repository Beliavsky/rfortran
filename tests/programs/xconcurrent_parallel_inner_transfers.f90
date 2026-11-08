! CYCLE and EXIT affect only the inner serial loop, not the worker iteration.
implicit none
integer, parameter :: n=9001
integer :: i,j,total,result(n)
do concurrent(i=1:n) local(j,total)
   total=0
   do j=1,6
      if(j==2) cycle
      if(j==5) exit
      total=total+j*i
   end do
   result(i)=total
end do
do i=1,n
   if(result(i)/=8*i) stop 1
end do
print *,"PASS"
end
