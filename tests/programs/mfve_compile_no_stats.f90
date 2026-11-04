! Compile-time control copied from C:/fortran/2015/vol_pred.f90.
! Only module renaming and removal of the two PRINT_STATS calls in
! FWD_VOL_EMA_PRED differ from that source. Other procedures are retained.
module fve_compile_no_stats_mod
use kind_mod      , only: dp
use util_mod      , only: default,set_optional_alloc,set_alloc
use statistics_mod, only: print_stats,obs_per_year,mean,rms
use correl_mod    , only: correl
use moving_average_mod, only: ema
implicit none
private
public :: vol_ema_pred, fwd_vol_ema_pred, qlike
integer, parameter :: nlen=20, nmetrics=5, irmse=1, iqlike=4, imse=5
character (len=nlen), parameter :: metric_names(nmetrics) = [character(len=nlen) :: &
   "rmse(HV,FV)", "corr(HV,FV)", "log corr(HV,FV)", "QLIKE", "MSE"]
real(kind=dp), parameter :: mse_scale = 0.001_dp
contains
subroutine vol_ema_pred(vol_fwd,xret,sym,i1,lambda,ret_shift,twist,wgt_avg, &
                        cstats,avg_corr_log_vol)
! compute exponentially weighted volatility and its correlation with future realized volatility
real(kind=dp)    , intent(in)           :: xret(:,:) ! returns
real(kind=dp)    , intent(in)           :: lambda ! smoothing for exponential moving average of volatility
character (len=*), intent(in)           :: sym(:)
integer          , intent(in)           :: i1
real(kind=dp)    , intent(in), optional :: ret_shift ! return shift in squared returns used to forecast vol
real(kind=dp)    , intent(in), optional :: twist     ! squared returns multiplied by (1+twist), (1-twist) for negative and positive returns
real(kind=dp)    , intent(in), optional :: wgt_avg   ! weight to give cross-sectional average in predicting volatility
character (len=*), intent(in), optional :: cstats(:)
real(kind=dp)    , intent(in), target   :: vol_fwd(:,:)
real(kind=dp)    , allocatable, target  :: hvol(:,:)
real(kind=dp)    , intent(out), optional :: avg_corr_log_vol
real(kind=dp)    , allocatable          :: ret_sq(:,:),shifted_ret(:,:)
integer                                 :: istock,nstocks,ndays_vol_pred
character (len=10), allocatable         :: cstats_(:)
logical                                 :: print_stats_retsq,print_stats_hvol
real(kind=dp), allocatable              :: corr_vol(:),corr_log_vol(:),rmse_vol(:), &
                                           hvol_avg(:)
real(kind=dp), pointer                  :: vol_hist_stk(:),vol_fwd_stk(:)
real(kind=dp)                           :: ret_shift_,twist_,wgt_avg_
wgt_avg_ = default(0.0_dp,wgt_avg)
ret_shift_ = default(0.0_dp,ret_shift)
twist_     = default(0.0_dp,twist)
ndays_vol_pred = size(vol_fwd,1)
print_stats_retsq = .false. 
print_stats_hvol  = .false.
write (*,"(/,*(a10,1x))") "lambda","ret_shift","twist","wgt_avg"
write (*,"(*(f10.4,1x))") lambda,ret_shift_,twist_,wgt_avg_
call set_optional_alloc(cstats_,["mean","sd  ","min ","max "],cstats)
nstocks = size(xret,2)
call set_alloc(xret-default(0.0_dp,ret_shift),shifted_ret)
if (twist_ /= 0.0_dp) then
   call set_alloc(merge(1+twist_,1-twist_,shifted_ret<0)*shifted_ret**2,ret_sq)
else
   call set_alloc(shifted_ret**2,ret_sq)
end if
if (print_stats_retsq) then
   call print_stats(cstats_,ret_sq,labels=sym,title="squared returns", &
                    print_num_obs=.true.,fmt_trailer="()")
end if
call set_alloc(sqrt(obs_per_year*ema(ret_sq,lambda)),hvol)
call set_alloc(sum(hvol,dim=2)/nstocks,hvol_avg)
forall (istock=1:nstocks) hvol(:,istock) = wgt_avg_*hvol_avg + (1-wgt_avg_)*hvol(:,istock)
if (print_stats_hvol) then
   call print_stats(cstats_,hvol,labels=sym,title="ema vol", &
                    print_num_obs=.true.,fmt_trailer="()")
end if
allocate (corr_vol(nstocks),corr_log_vol(nstocks),rmse_vol(nstocks))
do istock=1,nstocks
   vol_hist_stk => hvol(i1:ndays_vol_pred,istock)
   vol_fwd_stk  => vol_fwd(i1:ndays_vol_pred,istock)
   rmse_vol(istock)     = rms(log(vol_hist_stk)-log(vol_fwd_stk))
   corr_vol(istock)     = correl(vol_hist_stk,vol_fwd_stk)
   corr_log_vol(istock) = correl(log(vol_hist_stk),log(vol_fwd_stk))
end do
write (*,"(7x,*(a8))") "stock","avg",(trim(sym(istock)),istock=1,nstocks)
write (*,"('    rmse(HV,FV)',*(f8.3))") mean(    rmse_vol),rmse_vol
write (*,"('    corr(HV,FV)',*(f8.3))") mean(    corr_vol),corr_vol
write (*,"('log corr(HV,FV)',*(f8.3))") mean(corr_log_vol),corr_log_vol
if (present(avg_corr_log_vol)) avg_corr_log_vol = mean(corr_log_vol)
end subroutine vol_ema_pred
!
subroutine fwd_vol_ema_pred(sym, xret, ndays_fwd_vec, i1, lambda_vec, cstats, print_stats_hvol)
! for various forward vol horizons, find the optimal EWMA smoothing according to various metrics
character (len=*), intent(in)       :: sym(:)    ! (nstocks)
real(kind=dp)    , intent(in)       :: xret(:,:) ! (nret, nstocks) returns
integer          , intent(in)       :: i1        ! first observation to use in forward vols
integer          , intent(in)       :: ndays_fwd_vec(:) ! forward vol horizons
real(kind=dp)    , intent(in)       :: lambda_vec(:)    ! lambdas to try in exponential smoothing
character (len=*), intent(in)       :: cstats(:)
logical          , intent(in)       :: print_stats_hvol
real(kind=dp), allocatable, target  :: vol_fwd(:,:), hvol(:,:)
real(kind=dp)     , pointer         :: vol_hist_stk(:),vol_fwd_stk(:)
real(kind=dp), allocatable          :: metrics_mean(:,:), ret_sq(:,:), metrics(:,:)
integer                             :: ifwd, ibest, ilambda, imetric, iret, istock, &
                                       ndays_fwd, ndays_vol_pred, nfwd, nlambda, &
                                       nret, nstocks
real(kind=dp)                       :: lambda
logical                             :: write_each_lambda_
write_each_lambda_ = .false.
nfwd = size(ndays_fwd_vec)
nstocks = size(xret, 2)
nret = size(xret, 1)
nlambda = size(lambda_vec)
ret_sq = xret**2
allocate (metrics_mean(nlambda, nmetrics), metrics(nstocks, nmetrics))
loop_ifwd: do ifwd=1,nfwd
   ndays_fwd = ndays_fwd_vec(ifwd)
   write (*,"(/,a,i0)") "ndays_fwd = ", ndays_fwd
   if (ndays_fwd > nret) then
      write (*,*) "need ndays_fwd <= nret, STOPPING"
      stop
   end if
   ndays_vol_pred = nret-ndays_fwd
   allocate (vol_fwd(ndays_vol_pred,nstocks))
   do iret=1,ndays_vol_pred
      forall (istock=1:nstocks) vol_fwd(iret,istock) = sqrt(obs_per_year)*rms(xret(iret+1:iret+ndays_fwd,istock))
   end do
   do ilambda=1,nlambda
      lambda = lambda_vec(ilambda)
      if (write_each_lambda_) write (*,"(/,a,f8.4)") "lambda ",lambda
      hvol = sqrt(obs_per_year*ema(ret_sq,lambda))
      if (print_stats_hvol) then
      end if
      do istock=1,nstocks
         vol_hist_stk => hvol(i1:ndays_vol_pred,istock)
         vol_fwd_stk  => vol_fwd(i1:ndays_vol_pred,istock)
         metrics(istock, :) = [rms(log(vol_hist_stk)-log(vol_fwd_stk)), &
                               correl(vol_hist_stk,vol_fwd_stk), &
                               correl(log(vol_hist_stk),log(vol_fwd_stk)), &
                               mean(qlike(vol_fwd_stk**2, vol_hist_stk**2)), &
                               mse_scale * mean(mse_var(vol_fwd_stk**2, vol_hist_stk**2))]
      end do
      metrics_mean(ilambda, :) = [(mean(metrics(:,imetric)), imetric=1,nmetrics)]
      if (write_each_lambda_) then
         write (*,"(7x,*(a8))") "stock","avg",(trim(sym(istock)),istock=1,nstocks)
         do imetric=1,nmetrics
            write (*,"(a15,*(f8.3))") trim(metric_names(imetric)), &
               metrics_mean(ilambda, imetric), metrics(:, imetric)
         end do
      end if
   end do
   write (*,"(/,a15,a8,*(f8.4))") "metric", "best", lambda_vec
   do imetric=1,nmetrics
      if (any(imetric == [irmse, iqlike, imse])) then
         ibest = minloc(metrics_mean(:,imetric),dim=1)
      else
         ibest = maxloc(metrics_mean(:,imetric),dim=1)
      end if
      write (*,"(a15, *(f8.4))") trim(metric_names(imetric)), &
            lambda_vec(ibest),metrics_mean(:,imetric)
   end do
   deallocate (vol_fwd)
end do loop_ifwd
end subroutine fwd_vol_ema_pred

elemental function qlike(var_rlzd, var_est) result(y)
real(kind=dp), intent(in) :: var_rlzd, var_est
real(kind=dp)             :: y
real(kind=dp)             :: var_ratio
var_ratio = var_rlzd/var_est
y = var_ratio - log(var_ratio) - 1
end function qlike

elemental function mse_var(var_rlzd, var_est) result(y)
real(kind=dp), intent(in) :: var_rlzd, var_est
real(kind=dp)             :: y
y = var_est * (var_rlzd/var_est - 1)**2
end function mse_var

end module fve_compile_no_stats_mod
