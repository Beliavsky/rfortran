! Compile-only reduction of C:/fortran/2015/xfwd_vol_ema_pred.f90.
! Uses the original separately compiled modules; no input files are needed to compile.
program probe
use kind_mod, only: dp
use date_mdy_mod, only: date_mdy
use read_mdy_alloc_mod, only: read_time_series
implicit none
real(dp), allocatable :: xx(:,:)
type(date_mdy), allocatable :: dates(:)
character(20), allocatable :: sym(:)
character(1000) :: filename
integer :: nsym, nskip
read *, filename, nsym, nskip
call read_time_series(filename,dates,xx,ncol=nsym,symbols=sym, &
                      nrow_skip=nskip,print_summary=.true.)
print *, size(xx)
end program
