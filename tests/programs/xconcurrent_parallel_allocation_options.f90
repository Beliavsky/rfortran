implicit none
integer::i,j,n,y(9000)
do concurrent(i=1:9000)
   y(i)=total(i-4*(i/4))
end do
do j=1,9000
   n=j-4*(j/4)
   if(y(j)/=2*n+2) stop 1
end do
print *,y(1:4),y(9000)
contains
pure integer function total(n)
integer,intent(in)::n
integer::istat
integer,allocatable::work(:)
total=-100
allocate(work(n),source=2,stat=istat)
if(istat/=0) return
allocate(work(n+1),stat=istat)
if(istat/=0) total=sum(work)
deallocate(work,stat=istat)
if(istat==0) total=total+1
deallocate(work,stat=istat)
if(istat/=0) total=total+1
end function
end
