program main
implicit none
integer, parameter :: n = 3
real :: y(n)
integer :: a(3)
y(1) = 10.0
y(2) = 20.0
y(3) = 30.0
a(1) = 1
a(2) = 2
a(3) = 3
print *, y
print *, y(2:)
print *, y(:2)
print *, y(:)
print *, y(2:2)
print *, y(::2)
print *, y(::-1)
print *, y(n:1:-1)
print *, y(n:1:-2)
print *, 'empty', y(3:2), 'end'
print *, y(3:2)
print *, 'mixed', y(:1), 99, a(2:)
end program main
