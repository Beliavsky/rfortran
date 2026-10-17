! Exercise generic EXCLUDE dispatch with a SHAPE expression as an actual.
! Expected output: 2 4.
module exclude_mod
implicit none
interface exclude
   module procedure exclude_vec_int
end interface
contains
pure function exclude_vec_int(ivec, iexcl) result(jvec)
integer, intent(in) :: ivec(:), iexcl(:)
integer :: jvec(size(ivec)-size(iexcl))
integer :: i, j
j = 0
do i = 1, size(ivec)
   if (all(iexcl /= i)) then
      j = j + 1
      jvec(j) = ivec(i)
   end if
end do
end function exclude_vec_int
end module exclude_mod

program xexclude
use exclude_mod, only: exclude
implicit none
real :: tensor(2,3,4)
! SHAPE returns an INTEGER vector; it does not read tensor's unset elements.
print *, exclude(shape(tensor), iexcl=[2])
end program xexclude
