! Warning:    could-be-concurrent
! Enabled by: -Wextra
! A DO loop whose iterations are independent could be DO CONCURRENT, which
! rfort can run in parallel: each iteration writes its own elements of the
! arrays it defines, and defines its scalars before reading them. Perfectly
! nested independent loops are suggested as one construct. A recurrence, an
! accumulation, I/O or a scalar used after the loop is not reported.
program could_be_concurrent
implicit none
integer, parameter :: n = 5
integer :: i, j
real :: x(n), y(n), a(n, n), t, total
x = [(real(i), i = 1, n)]
do i = 1, n                              ! expect: could-be-concurrent
   t = 2*x(i)
   y(i) = t + sqrt(x(i))
end do
do j = 1, n                              ! expect: could-be-concurrent
   do i = 1, n
      a(i, j) = x(i)*y(j)
   end do
end do
do i = 2, n
   y(i) = y(i-1) + x(i)
end do
total = 0
do i = 1, n
   total = total + y(i)
end do
print *, y, total, sum(a)
end program could_be_concurrent
