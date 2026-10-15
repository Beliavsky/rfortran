! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_formatted_output
use util_mod, only:print_matrix_simple,write_trim_vec,write_words
use kind_mod, only:dp
implicit none
real(dp)::a(2,2)
a=reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp],[2,2])
call print_matrix_simple(a,fmt_r='(2f8.2)',title='matrix')
call write_trim_vec(['boy ','girl'])
call write_trim_vec(['boy ','girl'],prefix='"',suffix='"')
call write_words('words:',['boy ','girl'],'(100(1x,a))')
end program test_formatted_output
