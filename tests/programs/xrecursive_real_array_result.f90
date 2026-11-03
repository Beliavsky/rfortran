! Reduced from moving_average.f90:57-70, used by mavg_files.txt.
! The recursive function result is REAL, not CHARACTER.
! Expected: 1 2 3.
module recursive_real_array_mod
implicit none
contains
recursive pure function copy_array(xx, limit) result(yy)
real, intent(in) :: xx(:)
integer, intent(in) :: limit
real :: yy(size(xx))
if (limit > size(xx)) then
   yy = copy_array(xx, size(xx))
   return
end if
yy = xx
end function copy_array
end module recursive_real_array_mod

program xrecursive_real_array_result
use recursive_real_array_mod, only: copy_array
implicit none
print '(3f6.1)', copy_array([1.0, 2.0, 3.0], 4)
end program xrecursive_real_array_result
