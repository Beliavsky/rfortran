! Warning:    compare-reals
! Enabled by: -Wextra
! == or /= with a REAL or COMPLEX operand. Rounding makes exact equality of
! computed values unreliable: 0.1 + 0.2 == 0.3 is false in binary floating
! point. Compare with a tolerance. Comparisons with an exact zero are not
! reported.
program compare_reals
implicit none
real :: x, y
x = 0.1 + 0.2
y = 0.3
if (x == y) print *, "equal"                 ! expect: compare-reals
if (abs(x - y) <= 1.0e-6) print *, "close"
if (x /= 0.0) print *, "nonzero"
end program compare_reals
