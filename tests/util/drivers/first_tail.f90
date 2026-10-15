! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_first_tail
use util_mod, only: first,tail
use kind_mod, only: dp
implicit none
print *,first([2,5]),tail([2,5]),first([integer::]),tail([integer::])
print *,first([2.0_dp,5.0_dp]),tail([2.0_dp,5.0_dp])
print *,first([real(dp)::]),tail([real(dp)::])
print *,first(['cat','dog']),first([character(3)::])
end program test_first_tail
