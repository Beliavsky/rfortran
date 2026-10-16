! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_sorting
use util_mod, only:indexx,indexx_order,quick_sort
use kind_mod, only:dp
implicit none
real(dp)::x(10),empty(0),one(1),nan
integer::order(10),empty_order(0),one_order(1)
print *,indexx([3,1,2,4])
print *,indexx([3.0_dp,1.0_dp,2.0_dp,4.0_dp])
print *,indexx_order([3.0_dp,1.0_dp,2.0_dp,4.0_dp],'ascending')
print *,indexx_order([3.0_dp,1.0_dp,2.0_dp,4.0_dp],'descending')
print *,indexx_order([3.0_dp,1.0_dp,2.0_dp,4.0_dp],'none')
x=[5.0_dp,2.0_dp,9.0_dp,-1.0_dp,2.0_dp,7.0_dp,0.0_dp,3.0_dp,8.0_dp,4.0_dp]
call quick_sort(x,order)
print *,x
print *,order
call quick_sort(empty,empty_order)
one=4.0_dp
call quick_sort(one,one_order)
print *,size(empty_order),one,one_order
! NaN inputs return the original ordering before entering the sort loops.
nan=sqrt(-abs(x(1)))
print *,indexx([1.0_dp,nan,-1.0_dp]),isnan(nan)
end program test_sorting
