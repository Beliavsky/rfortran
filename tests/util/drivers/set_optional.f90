! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_set_optional
use util_mod, only:set_optional
use kind_mod, only:dp
implicit none
integer::i
real(dp)::r
logical::l
character(5)::s
call set_optional(i,2)
print *,i
call set_optional(i,2,5)
call set_optional(r,2.0_dp,5.0_dp)
call set_optional(l,.false.,.true.)
call set_optional(s,'cat','dog')
print *,i,r,l,trim(s)
end program test_set_optional
