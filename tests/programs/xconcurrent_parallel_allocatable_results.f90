implicit none
integer :: i,j,y(9000),n
do concurrent(i=1:9000)
   y(i)=sum(values(i-4*(i/4),i))
end do
do j=1,9000
   n=j-4*(j/4)
   if(y(j)/=n*j) stop 1
end do
print *,y(1:4),y(9000)
contains
pure function values(n,k) result(r)
integer,intent(in)::n,k
integer,allocatable::r(:)
allocate(r(-1:n-2))
r=k
end function
end
