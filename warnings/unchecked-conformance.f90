! Warning:    unchecked-conformance
! Enabled by: -Wextra
! A procedure that combines two assumed-shape arrays in a way that needs
! their sizes to match, as x*y or dot_product(x, y), without checking them.
! A caller that passes arrays of different sizes gets undefined results.
program unchecked_conformance
implicit none
print *, sumproduct([1.0, 2.0], [3.0, 4.0])
print *, checked_sumproduct([1.0, 2.0], [3.0, 4.0])
contains
function sumproduct(x, y) result(s)
real, intent(in) :: x(:), y(:)    ! expect: unchecked-conformance
real :: s
s = sum(x*y)
end function sumproduct
function checked_sumproduct(x, y) result(s)
real, intent(in) :: x(:), y(:)
real :: s
if (size(x) /= size(y)) error stop "checked_sumproduct: x and y differ in size"
s = sum(x*y)
end function checked_sumproduct
end program unchecked_conformance
