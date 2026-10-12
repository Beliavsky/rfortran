! Reproduce the CHARACTER dummy-argument blocker in util.f90:combine_vec_mat_real.
! Expected output: 1.0 2.0, followed by -1.0 -2.0.
program test_numeric_array_char_arg
implicit none
print '(2f6.1)', orient([1.0, 2.0], 'before')
print '(2f6.1)', orient([1.0, 2.0], 'after')
contains
function orient(x, position) result(y)
real, intent(in) :: x(:)
character(len=*), intent(in) :: position
real :: y(size(x))
if (position == 'before') then
   y = x
else
   y = -x
end if
end function orient
end program test_numeric_array_char_arg
