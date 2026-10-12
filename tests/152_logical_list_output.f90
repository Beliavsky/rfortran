program test_logical_list_output
implicit none
logical, parameter :: disabled = .false.
logical :: x, y, flags(2)
x = .true.
y = .false.
flags = [x, y]
print *, disabled
print '(l1)', disabled
print *, x, y
print *, flags
print *, .not.x, x .eqv. y, 2 > 1
write(*,*) x, y
print '(*(l1))', flags
! These are CHARACTER values, so their spelling must be preserved.
print *, '.TRUE.', '.FALSE.'
end program test_logical_list_output
