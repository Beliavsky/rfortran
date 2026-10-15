! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_all_equal
use util_mod, only:all_equal,check_all_equal
use kind_mod, only:dp
implicit none
print *,all_equal([integer::]),all_equal([2]),all_equal([2,2]),all_equal([2,5])
print *,all_equal([1,2],[1,2]),all_equal([1,2],[1,3]),all_equal([1,2],[1,3],iexcl=[2])
print *,all_equal([1.0_dp,2.0_dp],[1.0_dp,2.01_dp],tol=0.1_dp)
call check_all_equal([3,3],'test')
print *,'checked'
end program test_all_equal
