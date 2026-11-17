! Warning:    merge-evaluation
! Enabled by: -Weverything
! A MERGE argument that computes something, as x + 1 or f(x). MERGE may
! evaluate both arguments whatever the mask is, so the unselected one is
! computed for nothing, or fails. An IF construct, or a Fortran 2023
! conditional expression, evaluates only the selected value.
program merge_evaluation
implicit none
real :: x, y, low, high
x = 2.0
low = 0.0
high = 1.0
y = merge(sqrt(x), -x, x > 0)       ! expect: merge-evaluation
print *, y
y = merge(high, low, x > 1)
print *, y
end program merge_evaluation
