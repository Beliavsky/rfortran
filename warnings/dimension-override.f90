! Warning:    dimension-override
! Enabled by: -Wextra
! An entity whose own shape or length overrides the DIMENSION attribute or
! the CHARACTER length of its declaration, as y(3) in
! `real, dimension(2) :: x, y(3)`. It is legal, but a reader expects every
! entity of the statement to have the shape the statement shows.
program dimension_override
implicit none
real, dimension(2) :: x, y(3)          ! expect: dimension-override
real, dimension(2) :: u, v
x = 1.0
y = 2.0
u = 3.0
v = 4.0
print *, sum(x) + sum(y) + sum(u) + sum(v)
end program dimension_override
