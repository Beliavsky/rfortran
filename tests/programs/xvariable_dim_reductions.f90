program variable_dim_reductions
implicit none
integer, allocatable :: a(:,:), totals(:)
logical, allocatable :: mask(:,:), selected(:)
integer :: n, dim
n = 2
allocate(a(n,3), mask(n,3))
a = reshape([1,2,3,4,5,6], [n,3])
mask = a > 3
do dim = 1, 2
   totals = sum(a, dim=dim)
   selected = any(mask, dim=dim)
   print *, dim, totals
   print *, selected, count(mask, dim=dim)
end do
end program
