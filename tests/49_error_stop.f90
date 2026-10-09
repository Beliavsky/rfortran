implicit none
real(8), parameter :: weights(2) = [0.25d0, -0.5d0]
if (any(weights < 0.0d0)) error stop 'negative weight'
print *, 'unreachable'
end
