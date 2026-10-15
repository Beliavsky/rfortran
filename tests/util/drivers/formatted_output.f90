! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_formatted_output
use util_mod, only:print_matrix_simple,write_trim_vec,write_words
use kind_mod, only:dp
implicit none
real(dp)::a(2,2),empty(0,0)
a=reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp],[2,2])
call print_matrix_simple(a,fmt_r='(2f8.2)',title='matrix', &
   fmt_header='("header")',fmt_trailer='("trailer")')
call print_matrix_simple(empty,fmt_r='(2f8.2)',title='empty')
call write_trim_vec(['boy ','girl'])
call write_trim_vec(['boy ','girl'],prefix='"',suffix='"')
call write_words('words:',['boy ','girl'],'(100(1x,a))')
call write_trim_vec([character(4)::])
call write_trim_vec(["a'b ",'x"y ','    '])
call write_words('',[character(4)::],'(100(1x,a))')
end program test_formatted_output
