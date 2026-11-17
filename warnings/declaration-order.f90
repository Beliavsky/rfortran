! Warning:    declaration-order
! Enabled by: -Wstyle
! A procedure that declares a local variable before its dummy arguments or
! function result. Declaring the interface first, then the locals, lets a
! reader see how to call the procedure at a glance.
program declaration_order
implicit none
print *, mean([1.0, 2.0, 3.0]), mean2([1.0, 2.0, 3.0])
contains
function mean(x) result(m)
integer :: n
real, intent(in) :: x(:)            ! expect: declaration-order
real :: m
n = size(x)
m = sum(x)/n
end function mean
function mean2(x) result(m)
real, intent(in) :: x(:)
real :: m
integer :: n
n = size(x)
m = sum(x)/n
end function mean2
end program declaration_order
