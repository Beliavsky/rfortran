! Reproduce util.f90:read_vec_real's local CHARACTER length based on a dummy.
! Expected output: 1,2,3 and 4,5.
module dummy_char_length
implicit none
contains
subroutine normalize(text, cdelim, normalized)
character(len=*), intent(in) :: text, cdelim
character(len=*), intent(out) :: normalized
character(len=len(text)) :: mod_text
integer :: i
mod_text = text
do i = 1, len(text)
   if (text(i:i) == cdelim) mod_text(i:i) = ','
end do
normalized = mod_text
end subroutine normalize
end module dummy_char_length

program test_dummy_char_length
use dummy_char_length
implicit none
character(5) :: normalized
call normalize('1;2;3', ';', normalized)
print '(a)', trim(normalized)
call normalize('4|5', '|', normalized)
print '(a)', trim(normalized)
end program test_dummy_char_length
