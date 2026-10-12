! Reproduce the constructor implied-DO blocker in util.f90:compare_vec_strings.
! Expected output: F T F, followed by 1.
program test_constructor_implied_do
implicit none
call compare(["aa", "bb", "cc"], ["cc", "aa"])
contains
subroutine compare(xx, yy)
character(len=*), intent(in) :: xx(:), yy(:)
logical :: x_not_in_y(size(xx))
integer :: i
x_not_in_y = [(all(yy /= xx(i)), i=1,size(xx))]
print "(*(1x,l1))", x_not_in_y
print *, count(x_not_in_y)
end subroutine compare
end program test_constructor_implied_do
