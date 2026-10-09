program main
implicit none
real :: x(4), p
integer :: a(4), i
x(1) = -2.0
x(2) = 1.0
x(3) = 3.0
x(4) = 4.0
a(1) = -2
a(2) = 1
a(3) = 3
a(4) = 4
print *, sum(x), sum(x**2), minval(x), maxval(x), product(x)
print *, sum(a), sum(a**2), minval(a), maxval(a), product(a)
print *, sum(x(2:)), product(x(2:3)), sum(x(4:1:-2))
print *, sum(x(3:2)), product(x(3:2))
print *, sum(x + 2.0), sum(x*x), sum(x(1:2) + x(3:4))
p = 1.0
do i = 1,4
   p = p*x(i)
end do
print *, p, product(x)
end program main
