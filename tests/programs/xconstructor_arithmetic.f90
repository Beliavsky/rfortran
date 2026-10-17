! Reproducer for util.f90's init_random_seed constructor expression.
! Expected output: 100 137 174
program test_constructor_arithmetic
implicit none
integer :: i, n, clock
integer, allocatable :: seed(:)

n = 3
clock = 100
allocate(seed(n))
seed = clock + 37 * [(i-1, i=1,n)]
if (any(seed /= [100,137,174])) stop 1
print *, seed
deallocate(seed)
end program test_constructor_arithmetic
