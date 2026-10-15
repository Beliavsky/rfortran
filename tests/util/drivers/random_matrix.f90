! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_random_matrix
use util_mod, only:random_matrix,ran_uni
use kind_mod, only:dp
implicit none
real(dp)::a(3,2),b(4)
a=random_matrix(3,2)
b=ran_uni(4)
print *,all(a>=0.0_dp),all(a<1.0_dp),all(b>=0.0_dp),all(b<1.0_dp)
print *,size(random_matrix(0,2))
end program test_random_matrix
