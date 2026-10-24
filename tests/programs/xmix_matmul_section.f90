implicit none
real(8), allocatable :: l(:,:,:), x(:,:)
real(8) :: z(2)
integer :: j,n
n=2
allocate(l(n,n,2),x(2,n))
l(:,:,1)=reshape([1d0,2d0,3d0,4d0],[2,2])
l(:,:,2)=reshape([5d0,6d0,7d0,8d0],[2,2])
z=[1d0,2d0]
do j=1,2
x(j,:)=[10d0,20d0]+matmul(l(:,:,j),z)
end do
print *,x
end
