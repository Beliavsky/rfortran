! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_optional_values
use util_mod, only: optional_value,if_else,and
use kind_mod, only: dp
implicit none
print *,optional_value(2),optional_value(2,5)
print *,optional_value(2.0_dp),optional_value(2.0_dp,5.0_dp)
print *,optional_value('cat'),optional_value('cat','dog')
print *,optional_value(.false.),optional_value(.false.,.true.)
print *,if_else(.true.,2,5),if_else(.false.,2,5)
print *,if_else(.true.,2.0_dp,5.0_dp),if_else(.false.,2.0_dp,5.0_dp)
print *,and(),and(.true.),and(.false.),and(.true.,.false.),and(.true.,.true.)
end program test_optional_values
