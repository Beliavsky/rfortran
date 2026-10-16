program p1
implicit none
integer :: n, i
real(kind=8), allocatable :: x(:), y(:,:)
n = 5
allocate(x(n), y(n,n))
do i=1,n
  x(i) = i**2
end do
y = spread(x, 1, n)
print *, sum(x), maxval(y), size(y)
deallocate(x)
allocate(x(2*n))
x = 1.0d0
print *, sum(x)
end program
