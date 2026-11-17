! Warning:    assumed-size
! Enabled by: -Wstyle
! An assumed-size dummy array, as x(*). Its size is unknown inside the
! procedure, so whole-array operations and bounds checks cannot be used.
! An assumed-shape array, x(:), carries its shape.
program assumed_size
implicit none
real :: a(3) = [1.0, 2.0, 3.0]
print *, total(a, 3), total_shape(a)
contains
function total(x, n) result(s)
integer, intent(in) :: n
real, intent(in) :: x(*)            ! expect: assumed-size
real :: s
s = sum(x(1:n))
end function total
function total_shape(x) result(s)
real, intent(in) :: x(:)
real :: s
s = sum(x)
end function total_shape
end program assumed_size
