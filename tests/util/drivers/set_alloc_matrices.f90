! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_set_alloc_matrices
use util_mod, only:set_alloc
use kind_mod, only:dp
implicit none
integer,allocatable::i(:,:)
real(dp),allocatable::r(:,:),t(:,:,:)
logical,allocatable::l(:,:),q(:,:,:)
character(3),allocatable::s(:,:)
call set_alloc(reshape([1,2,3,4],[2,2]),i)
call set_alloc(reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp],[2,2]),r)
call set_alloc(reshape([2.0_dp,5.0_dp],[1,2,1]),t)
call set_alloc(reshape([.true.,.false.],[1,2]),l)
call set_alloc(reshape([.true.,.false.],[1,2,1]),q)
call set_alloc(reshape(['cat','dog'],[1,2]),s)
print *,shape(i),i,r,t,l,q,s
end program test_set_alloc_matrices
