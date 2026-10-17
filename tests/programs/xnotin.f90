! Exercise util.f90's .notin. operator with a scalar and an array.
! Expected output: F T.
module membership_mod
implicit none
interface operator(.notin.)
   module procedure int_not_in_vec
end interface
contains
pure function int_not_in_vec(i, ivec) result(tf)
integer, intent(in) :: i, ivec(:)
logical :: tf
tf = all(ivec /= i)
end function int_not_in_vec
end module membership_mod

program xnotin
use membership_mod, only: operator(.notin.)
implicit none
integer :: values(3)
values = [1, 2, 3]
print *, 2 .notin. values, 4 .notin. values
end program xnotin
