! Compile-only probe narrowing the fve statistics-reporting call path.
program probe
use kind_mod, only: dp
use statistics_mod, only: stats
implicit none
real(dp), allocatable :: x(:)
real(dp), allocatable :: y(:)
character(20), allocatable :: names(:)
integer :: nstats
integer :: n
read *, n
allocate(x(n))
x=0.01_dp
read *,nstats
allocate(names(nstats))
names="mean"
y=stats(names,x)
print *,sum(y)
y=stats(names,x)
print *,sum(y)
y=stats(names,x)
print *,sum(y)
y=stats(names,x)
print *,sum(y)
end program
