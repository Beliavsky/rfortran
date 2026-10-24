! Regression for MINVAL(xx,DIM=1) in xacv.f90.
! Expected values (real formatting may differ):
! 11 21 31
! 31 32
! 23 43 63
program runtime_dim_reduction
implicit none
real, allocatable :: xx(:,:)
integer :: n,m,i,j
n=2
m=3
allocate(xx(n,m))
do j=1,m
   do i=1,n
      xx(i,j)=10*j+i
   end do
end do
print *, minval(xx,dim=1)
print *, maxval(xx,dim=2)
print *, sum(xx,dim=1)
end program
