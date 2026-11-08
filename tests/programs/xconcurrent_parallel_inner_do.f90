! Serial inner DO with iteration-private control and accumulator.
implicit none
integer, parameter :: n=9001, m=3
integer :: i,j,total,a(m,n),result(n)
do i=1,n
   a(:,i)=i
end do
do concurrent(i=1:n) local(j,total)
   total=0
   do j=1,m
      total=total+a(j,i)
   end do
   result(i)=total
end do
do i=1,n
   if(result(i)/=m*i) stop 1
end do
print *,"PASS"
end
