implicit none
integer :: v(3), i
real :: r(2)
double precision :: d(2)
v = 10
print *, v
v = v(1) + 1
print *, v
print *, sum(v)
r = 1.5
d = 2.5
print *, r, d
do i = 1, 2
  v = i
end do
print *, v
v = sum(v)
print *, v
end
