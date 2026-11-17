! Warning:    maybe-out-of-bounds
! Enabled by: -Wextra
! A constant subscript of a PACK result, which may have fewer elements than
! the subscript needs: pack(x, mask) is empty when no element of the mask
! is true.
program maybe_out_of_bounds
implicit none
real :: x(5) = [3.0, -1.0, 4.0, -1.0, 5.0]
real, allocatable :: pos(:), neg(:)
pos = pack(x, x > 0)
neg = pack(x, x < 0)
print *, pos(1)            ! expect: maybe-out-of-bounds
if (size(neg) > 0) print *, neg(1)
end program maybe_out_of_bounds
