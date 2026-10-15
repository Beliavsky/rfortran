! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_combinatorics
use util_mod, only:factorial,factorial_long,choose,choose_func,choose_func_long
use kind_mod, only:i64
implicit none
print *,factorial([-1,0,1,2,3,4]),factorial_long(10_i64)
print *,choose(4,[0,1,2,3,4,5])
! General recursive cases read the original module's uninitialized private mm
! cache. Test defined boundary paths here; see ../source-issues.md.
print *,choose_func(5,0),choose_func(5,1),choose_func(5,5)
print *,choose_func(5,8),choose_func(5,-1),choose_func_long(5_i64,5_i64)
end program test_combinatorics
