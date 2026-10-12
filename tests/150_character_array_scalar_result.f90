! Reproduce the CHARACTER array dummy blocker in util.f90:num_unique_char.
! Expected output: 2, followed by 4.
program test_character_array_scalar_result
implicit none
print *, total_length(['a ', 'b '])
print *, total_length(['ab', 'cd'])
contains
function total_length(xx) result(n)
character(len=*), intent(in) :: xx(:)
integer :: n, i
n = 0
do i = 1, size(xx)
   n = n + len_trim(xx(i))
end do
end function total_length
end program test_character_array_scalar_result
