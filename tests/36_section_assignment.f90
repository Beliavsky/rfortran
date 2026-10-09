implicit none
integer :: v(3), i
real :: r(3)
v(1:2) = 10
print *, v(1:2)
v(3:) = 20
print *, v
v(3:1:-2) = v(1) + 1
print *, v
v(:) = 7
v(2:1) = 99
print *, v
do i = 1, 2
  v(::2) = i
end do
print *, v
r(:2) = 1.5
r(3:3) = 2.5
print *, r
end
