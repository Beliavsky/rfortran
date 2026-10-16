program b3
implicit none
integer, parameter :: n = 400
real(8) :: a(n,n), b(n,n), c(n,n)
integer :: i, j, k
do j = 1, n
  do i = 1, n
    a(i,j) = 1.0d0/(i+j); b(i,j) = 1.0d0*(i-j)/n
  end do
end do
c = 0
do j = 1, n
  do k = 1, n
    do i = 1, n
      c(i,j) = c(i,j) + a(i,k)*b(k,j)
    end do
  end do
end do
print *, sum(c)
end program
