! Warning:    maybe-uninitialized
! Enabled by: -Wextra
! A local variable read where some path from the start of the procedure
! reaches the read without assigning it, so its value may be undefined.
program maybe_uninitialized
implicit none
integer :: n
real :: x, y
n = 3
if (n > 2) then
   x = 1.0
end if
print *, x            ! expect: maybe-uninitialized
if (n > 2) then
   y = 1.0
else
   y = 0.0
end if
print *, y
end program maybe_uninitialized
