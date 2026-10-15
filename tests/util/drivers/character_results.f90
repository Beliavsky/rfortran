! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_character_results
use util_mod, only: operator(+),trim_append,munge,concat_char_vec,join_fixed
implicit none
print *,trim('cat '+' dog')
print *,trim(trim_append('cat ','dog'))
print *,munge(['a','b'],['x','y'],':')
print *,concat_char_vec(['dog','cat'],['four','five'])
print *,concat_char_vec([character(3)::],['four','five'])
print *,join_fixed(['boy ','girl','a   '],10)
end program test_character_results
