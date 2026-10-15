! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_optional_character_results
use util_mod, only:vec,default_alloc
implicit none
print *,size(vec())
print '(100(a,1x))',vec('one')
print '(100(a,1x))',vec('one','two','three')
print '(100(a,1x))',vec('1','2','3','4','5','6','7','8','9','10')
print '(100(a,1x))',default_alloc(['boy ','girl'])
print '(100(a,1x))',default_alloc(['boy ','girl'],['cat','dog'])
end program test_optional_character_results
