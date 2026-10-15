! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_repeat_values
use util_mod, only:repeat,zeros
use kind_mod, only:dp
implicit none
print *,repeat(3,2),repeat(2,2.5_dp),repeat(2,.true.)
print *,repeat(2,'cat')
print *,repeat(2,[1.0_dp,2.0_dp])
print *,repeat(2,['a','b'])
print *,zeros(3),size(repeat(0,2)),size(zeros(0))
end program test_repeat_values
