! Warning:    maybe-nonconforming
! Enabled by: -Wextra
! An elemental operation on two arrays whose sizes depend on the data and
! may differ, as two PACK results with different masks. Operands that do not
! conform give undefined results.
program maybe_nonconforming
implicit none
real :: x(6) = [1.0, -2.0, 3.0, -4.0, 5.0, 6.0]
real, allocatable :: pos(:), neg(:), big(:)
pos = pack(x, x > 0)
neg = pack(x, x < 0)
big = pack(x, x > 0)
print *, pos - neg         ! expect: maybe-nonconforming
print *, pos + big
end program maybe_nonconforming
