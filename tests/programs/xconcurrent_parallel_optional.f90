! Read-only optional arguments are specialized before parallel worker generation.
module optional_score_mod
implicit none
contains
pure function score(i, offset, weights) result(r)
integer, intent(in) :: i
real(kind=kind(0d0)), intent(in), optional :: offset, weights(:)
real(kind=kind(0d0)) :: r
r = real(i, kind=kind(0d0))
if (present(offset)) r = r + offset
if (present(weights)) r = r + sum(weights)
end function
end module

program test_optional_parallel
use optional_score_mod, only: score
implicit none
integer :: i
real(kind=kind(0d0)) :: a(9001), weights(3)
weights = [1d0, 2d0, 3d0]
do concurrent(i=1:size(a))
   a(i) = score(i) + score(i, weights=weights) + score(i, offset=2d0)
end do
do i=1,size(a)
   if (a(i) /= 3d0*i+8d0) stop 1
end do
print *, 'PASS'
end program
