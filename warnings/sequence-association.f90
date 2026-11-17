! Warning:    sequence-association
! Enabled by: -Wextra
! A whole array passed to an explicit-shape dummy of another shape. The
! dummy is associated with the leading elements of the actual in array
! element order, which is legal but rarely what is meant: here the 3x2
! dummy sees the first 6 of 12 elements, not the first rows or columns.
program sequence_association
implicit none
real :: a(4, 3), b(3, 2)
a = 1.0
b = 1.0
call show(a)                 ! expect: sequence-association
call show(b)
contains
subroutine show(m)
real, intent(in) :: m(3, 2)
print *, sum(m)
end subroutine show
end program sequence_association
