! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_unique_characters
use util_mod, only:unique_values
implicit none
character(5)::words(5)
character(5),allocatable::uniq(:)
integer,allocatable::freq(:),first(:)
integer::groups(5)
words=['adj  ','noun ','verb ','adj  ','noun ']
call unique_values(words,uniq,i1=first,freq=freq,igroup=groups)
print *,uniq,first,freq,groups
end program test_unique_characters
