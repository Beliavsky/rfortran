module matrix_operations
implicit none
contains
subroutine fill_matrix(x)
integer, intent(out) :: x(:,:)
integer :: i,j
do j=1,size(x,2)
do i=1,size(x,1)
x(i,j)=10*j+i
end do
end do
end subroutine fill_matrix
end module matrix_operations

program matrices
use matrix_operations, only: fill_matrix
implicit none
integer, allocatable :: a(:,:), b(:,:), tensor(:,:,:)
integer :: i,j
real(8) :: r(2,2)
allocate(a(2,3),b(2,3),tensor(2,2,2))
call fill_matrix(a)
b=a+1
print *, size(a),size(a,1),size(a,2)
print *, a
print *, b
print *, a(2,3)
i=1
j=3
print *, a(i,j)
tensor=7
print *, size(tensor),tensor(2,2,2),sum(tensor)
r=1.5d0
r(2,1)=2.5d0
print *, r,sum(r)
i=1
do while(a(i,1)<12)
i=i+1
end do
print *, i
end program matrices
