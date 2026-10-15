! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_set_alloc_vectors
use util_mod, only:set_alloc
use kind_mod, only:dp,i64
implicit none
integer,allocatable::i(:)
integer(i64),allocatable::j(:)
real(dp),allocatable::r(:)
logical,allocatable::l(:)
character(4),allocatable::s(:)
call set_alloc([2,5],i)
call set_alloc([2_i64,5_i64],j)
call set_alloc([2.0_dp,5.0_dp],r)
call set_alloc([.true.,.false.],l)
call set_alloc(['cat','dog'],s)
print *,i,j,r,l,s
call set_alloc([integer::],i)
print *,allocated(i),size(i)
call set_alloc([7],i)
print *,i
end program test_set_alloc_vectors
