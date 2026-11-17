! Warning:    format-reversion
! Enabled by: -Wextra
! An output list with more items than the format has data edit descriptors,
! so the format is reused from its last group and the output continues on
! new records. A repeat count, as (10f8.3), or an unlimited group, as
! (*(g0,1x)), shows it is intended and is not reported.
program format_reversion
implicit none
real :: x(3) = [1.0, 2.0, 3.0]
print "(f6.2)", x(1:3)           ! expect: format-reversion
print "(3f6.2)", x(1:3)
print "(*(f6.2))", x(1:3)
end program format_reversion
