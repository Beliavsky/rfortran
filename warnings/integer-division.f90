! Warning:    integer-division
! Enabled by: -Wall
! An integer division that discards a remainder where a REAL result was
! probably wanted: a constant division such as 1/3, or an integer quotient
! converted to REAL. Write real(n)/m, or real(n/m) when truncation is meant.
program integer_division
implicit none
integer :: n, m
real :: ratio, third
n = 7
m = 2
third = 1/3              ! expect: integer-division
ratio = n/m              ! expect: integer-division
print *, third, ratio
ratio = real(n)/m
print *, ratio
ratio = real(n/m)
print *, ratio, n/m
end program integer_division
