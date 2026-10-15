! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_string_fields
use util_mod, only:quote,strip,num_fields,num_matching_char,ends_with
implicit none
print *,quote(' cat '),quote('')
print *,trim(strip(',boy,,girl,,,  ',',')),trim(strip(',,,',','))
print *,num_fields('boy,girl,,'),num_fields('boy,girl,,',xstrip=.false.)
print *,num_fields('a:b:c',delim=':')
print *,num_matching_char('a,b,,',','),num_matching_char('abc','xx')
print *,ends_with(['foo.bin ','foo.txt ','foo.bins','.bin.abc'],'.bin')
print *,ends_with('','.bin'),ends_with('abc','abcdef')
end program test_string_fields
