! Reproduce util.f90:gen's persistent initialized icomb counter.
! Expected output: 1, 2, 1, 2.
program test_save_counter
implicit none
call next_combination()
call next_combination()
call next_combination(.true.)
call next_combination(.false.)
contains
subroutine next_combination(zero_icomb)
logical, intent(in), optional :: zero_icomb
integer, save :: icomb = 0
if (present(zero_icomb)) then
   if (zero_icomb) icomb = 0
end if
icomb = icomb + 1
print *, icomb
end subroutine next_combination
end program test_save_counter
