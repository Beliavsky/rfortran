! Compilation-performance repro; single-precision SUM may round.
! Keep out of the default fast regression suite.
program slow_array_constructor
implicit none
integer, parameter :: n=10000
real :: x(n)
integer :: i
x = [(real(i),i=1,n)]
print *, sum(x)
end program
