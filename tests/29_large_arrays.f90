program main
implicit none
integer, parameter :: n = 20000
integer :: a(n), i
real :: b(n)
double precision :: c(n)
do i = 1,n
   a(i) = 1
   b(i) = 2.0
   c(i) = 3.0
end do
print *, sum(a), sum(b), sum(c)
print *, a(n), b(n), c(n)
end program main
