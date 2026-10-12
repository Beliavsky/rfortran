! Reproduce the CHARACTER array-result function blocker in util.f90:munge.
! Expected output: 'a-1' 'b-1' 'a-2' 'b-2'.
program test_munge
implicit none
print "(*(1x,a))", "'" // munge(["a", "b"], ["1", "2"], "-") // "'"
contains
function munge(xx, yy, delim) result(xy)
character(len=*), intent(in) :: xx(:), yy(:), delim
character(len=len(xx)+len(yy)+len(delim)) :: xy(size(xx)*size(yy))
integer :: ix, iy, nx
nx = size(xx)
forall (ix=1:size(xx), iy=1:size(yy)) &
   xy(ix+(iy-1)*nx) = trim(xx(ix)) // delim // trim(yy(iy))
end function munge
end program test_munge
