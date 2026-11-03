! Reference output: 4 3 3
program test_fold_sizes
use fve_fold_bounds_repro, only: fold_sizes
implicit none
integer :: sizes(3)
call fold_sizes(10,3,sizes)
print *,sizes
if (any(sizes /= [4,3,3])) stop 1
end program test_fold_sizes
