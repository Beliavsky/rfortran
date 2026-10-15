! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_sorting
use util_mod, only:indexx,indexx_order
use kind_mod, only:dp
implicit none
print *,indexx([3,1,2,4])
print *,indexx([3.0_dp,1.0_dp,2.0_dp,4.0_dp])
print *,indexx_order([3.0_dp,1.0_dp,2.0_dp,4.0_dp],'ascending')
end program test_sorting
