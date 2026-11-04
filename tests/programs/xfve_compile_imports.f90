! Compile-only reduction of C:/fortran/2015/xfwd_vol_ema_pred.f90.
! Uses the original separately compiled modules; no input files are needed to compile.
program probe
use read_mdy_alloc_mod, only: read_time_series
use statistics_mod, only: print_stats
use vol_pred_mod, only: fwd_vol_ema_pred
implicit none
print *, 'imports only'
end program
