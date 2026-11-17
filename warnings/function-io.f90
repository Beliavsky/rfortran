! Warning:    function-io
! Enabled by: -Wstyle
! A function that performs input/output on an external unit. The side
! effect keeps it from being PURE, and referencing it in an I/O statement
! on the same unit is recursive I/O, which is not allowed.
program function_io
implicit none
real :: a
a = area(2.0)
print *, a
print *, area_quiet(2.0)
contains
function area(r) result(a)
real, intent(in) :: r
real :: a
a = 3.14159*r*r
write (*, *) "area computed"          ! expect: function-io
end function area
pure function area_quiet(r) result(a)
real, intent(in) :: r
real :: a
a = 3.14159*r*r
end function area_quiet
end program function_io
