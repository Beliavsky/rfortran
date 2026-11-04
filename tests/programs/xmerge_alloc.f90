implicit none
real, allocatable :: x(:)
integer :: n
allocate(x(2))
n = merge(size(x), -1, allocated(x))
print*,n
end
