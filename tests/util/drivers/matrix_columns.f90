! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_matrix_columns
use util_mod, only:add_constant_columns,add_one_var
use kind_mod, only:dp
implicit none
real(dp)::a(2,1)
integer,allocatable::positions(:,:)
a(:,1)=[1.0_dp,2.0_dp]
print *,add_constant_columns(a,[5.0_dp,6.0_dp])
call add_one_var(4,[2,3],positions)
print *,shape(positions),positions
end program test_matrix_columns
