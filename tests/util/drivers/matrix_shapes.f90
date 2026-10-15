! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_matrix_shapes
use util_mod, only: matrix,vector,tensor
use kind_mod, only: dp
implicit none
real(dp)::a(2,2)
a=matrix([1.0_dp,2.0_dp,3.0_dp,4.0_dp],2)
print *,shape(matrix([1.0_dp,2.0_dp])),a,vector(a)
print *,matrix(2,3)
print *,shape(tensor(a)),tensor(a)
end program test_matrix_shapes
