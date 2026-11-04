! Compile-only probe narrowing the fve statistics-reporting call path.
program probe
use kind_mod, only: dp
use statistics_mod, only: stats
implicit none
real(dp), allocatable :: x(:)
real(dp) :: y
character(20) :: statistic
integer :: n
read *, n
allocate(x(n))
x=0.01_dp
read *,statistic
y=stats(statistic,x)
print *,y
end program
