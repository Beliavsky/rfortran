! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_string_matching
use util_mod, only:match_string
implicit none
print *,match_string('a',['b','a','a']),match_string('z',['a','b'])
print *,match_string(['b','z','a'],['a','b','a'])
print *,match_string('a',[character(1)::])
end program test_string_matching
