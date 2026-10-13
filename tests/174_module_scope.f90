! Reproducer: CHARACTER-input function reads a module CHARACTER array constant.
! Based on util.f90: ts_transform_matrix and str_trans.
! Expected output: 2
module transform_names
implicit none
character(10), parameter :: str_trans(3) = &
   ['ratio_diff', 'diff      ', 'none      ']
contains
integer function ts_transform_matrix(trans) result(j)
character(*), intent(in) :: trans
integer :: i
j=0
do i=1,size(str_trans)
   if (trans == trim(str_trans(i))) j=i
end do
end function ts_transform_matrix
end module transform_names

program test_transform_names
use transform_names
implicit none
print *,ts_transform_matrix('diff')
end program test_transform_names
