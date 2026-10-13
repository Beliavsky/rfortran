! Subscripted arrays must retain the rank of their section dimensions.
! Expected output: 108, 104, 102, 108 on separate lines.
module generic_sections_mod
implicit none
interface classify
   module procedure scalar_int
end interface
contains
integer function scalar_int(x) result(y)
integer, intent(in) :: x
y = 100 + x
end function scalar_int
end module generic_sections_mod

program xgeneric_sections
use generic_sections_mod
implicit none
integer :: a(2,2,2)
a = 1
print *, classify(sum(a(:,:,:)))
print *, classify(sum(a(:,:,1)))
print *, classify(sum(a(:,1,1)))
print *, classify(sum(reshape(a,[2,4])))
end program xgeneric_sections
