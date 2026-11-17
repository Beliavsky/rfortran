! Warning:    merge-guard
! Enabled by: -Wall
! MERGE may evaluate both values whatever the mask is, so the mask cannot
! guard them: merge(a(i), 0, i > 0) may still evaluate a(0). Use an IF
! construct, or a Fortran 2023 conditional expression.
program merge_guard
implicit none
integer :: a(3) = [10, 20, 30]
integer :: i, n, v
i = 0
n = 0
v = merge(a(i), 0, i > 0)          ! expect: merge-guard
print *, v
v = merge(100/n, 0, n /= 0)        ! expect: merge-guard
print *, v
v = 0
if (i > 0) v = a(i)
print *, v
end program merge_guard
