! Compile-only reduction of C:/fortran/2015/xfwd_vol_ema_pred.f90.
! Uses the original separately compiled modules; no input files are needed to compile.
program probe
use kind_mod, only: dp
use fve_compile_no_stats_mod, only: fwd_vol_ema_pred
implicit none
real(dp), allocatable :: xret(:,:), lambda_vec(:)
integer, allocatable :: ndays_fwd_vec(:)
character(20), allocatable :: sym(:), cstats(:)
integer :: n, m, nstats, nlambda, nfwd, i1
logical :: print_hvol
read *, n, m, nstats, nlambda, nfwd, i1, print_hvol
allocate(xret(n,m),sym(m),cstats(nstats),lambda_vec(nlambda),ndays_fwd_vec(nfwd))
xret=0.01_dp
sym='sample'
cstats='mean'
lambda_vec=0.94_dp
ndays_fwd_vec=1
call fwd_vol_ema_pred(sym,xret,ndays_fwd_vec,i1,lambda_vec,cstats,print_hvol)
end program
