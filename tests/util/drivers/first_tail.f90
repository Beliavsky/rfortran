! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_first_tail
use util_mod, only: first,tail
use kind_mod, only: dp
implicit none
character(3) :: empty_words(0)
print *,first([2,5]),tail([2,5]),first([integer::]),tail([integer::])
print *,first([2.0_dp,5.0_dp]),tail([2.0_dp,5.0_dp])
print *,first([real(dp)::]),tail([real(dp)::])
! The installed gfortran loses the length of an empty typed constructor in
! this call. Use a variable for the reference; test_first_tail.py also checks
! the empty constructor directly against its declared element length.
print *,first(['cat','dog']),first(empty_words)
end program test_first_tail
