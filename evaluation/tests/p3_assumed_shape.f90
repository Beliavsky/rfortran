module la
implicit none
contains
subroutine matvec(a, x, y)
  real(8), intent(in) :: a(:,:), x(:)
  real(8), intent(out) :: y(:)
  integer :: i
  do i = 1, size(a,1)
    y(i) = dot_product(a(i,:), x)
  end do
end subroutine
end module
program p3
use la
implicit none
integer :: n, i
real(8), allocatable :: a(:,:), x(:), y(:)
read_n: block
  n = 3
end block read_n
allocate(a(n,n), x(n), y(n))
a = reshape([(real(i,8), i=1,n*n)], [n,n])
x = 1
call matvec(a, x, y)
print *, y
print *, matmul(a, x)
end program
