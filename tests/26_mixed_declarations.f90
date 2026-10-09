program main
implicit none
integer, parameter :: n = 2, m = n + 1
real :: x, y(n), z, a(m)
integer :: b(2), k, c(1)
x = 10.0
y(1) = 20.0
y(2) = 30.0
z = 40.0
a(1) = 50.0
a(2) = 60.0
a(3) = 70.0
b(1) = 1
b(2) = 2
k = 3
c(1) = 4
print *, x, y, z, a
print *, b, k, c
end program main
