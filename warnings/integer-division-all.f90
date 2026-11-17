! Warning:    integer-division-all
! Enabled by: -Weverything
! Every integer division, which truncates toward zero: -7/2 is -3, while
! Python's -7//2 is -4. Useful when porting code to or from Python.
! (-Winteger-division, in -Wall, reports only the divisions that are likely
! mistakes.)
program integer_division_all
implicit none
integer :: n, m, q
real :: x
n = -7
m = 2
q = n/m                  ! expect: integer-division-all
x = real(n)/m
print *, q, x
end program integer_division_all
