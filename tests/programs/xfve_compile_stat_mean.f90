! Compile-only probe narrowing the fve statistics-reporting call path.
program probe
use kind_mod, only: dp
use statistics_mod, only: mean
implicit none
real(dp), allocatable :: x(:)
real(dp) :: y
integer :: n
read *, n
allocate(x(n))
x=0.01_dp
y=mean(x)
print *,y
end program
