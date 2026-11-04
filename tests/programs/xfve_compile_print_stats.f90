! Compile-only reduction of C:/fortran/2015/xfwd_vol_ema_pred.f90.
! Uses the original separately compiled modules; no input files are needed to compile.
program probe
use kind_mod, only: dp
use statistics_mod, only: print_stats
implicit none
real(dp), allocatable :: xret(:,:)
character(20), allocatable :: sym(:), cstats(:)
integer :: n, m, nstats
read *, n, m, nstats
allocate(xret(n,m),sym(m),cstats(nstats))
xret=0.01_dp
sym='sample'
cstats='mean'
call print_stats(cstats,xret,labels=sym,fmt_trailer="()")
end program
