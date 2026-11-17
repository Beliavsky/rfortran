! Warning:    conformance
! Enabled by: -Wall
! A call whose array arguments, of sizes known when compiling, break a size
! relationship that the procedure needs, as here x*y for x and y of
! different sizes.
program conformance
implicit none
print *, sumproduct([1.0, 2.0], [3.0, 4.0])
print *, sumproduct([1.0, 2.0], [3.0])          ! expect: conformance
contains
function sumproduct(x, y) result(s)
real, intent(in) :: x(:), y(:)
real :: s
s = sum(x*y)
end function sumproduct
end program conformance
