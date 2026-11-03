! Reduced from the metrics constructor in Fortran 2015 vol_pred.f90.
! Expected output: 0.565952 0.944911 0.866025 0.424196 0.003750
module pointer_metrics_mod
implicit none
integer, parameter :: dp=kind(1d0)
contains
pure function mean(x) result(y)
real(dp), intent(in) :: x(:)
real(dp) :: y
y=sum(x)/size(x)
end function
elemental function qlike(var_rlzd,var_est) result(y)
real(dp), intent(in) :: var_rlzd,var_est
real(dp) :: y,var_ratio
var_ratio=var_rlzd/var_est
y=var_ratio-log(var_ratio)-1
end function
elemental function mse_var(var_rlzd,var_est) result(y)
real(dp), intent(in) :: var_rlzd,var_est
real(dp) :: y
y=var_est*(var_rlzd/var_est-1)**2
end function
pure function correl(x,z) result(y)
real(dp), intent(in) :: x(:),z(:)
real(dp) :: y,xt(size(x)),zt(size(x))
xt=x-mean(x)
zt=z-mean(z)
y=dot_product(xt,zt)/sqrt(dot_product(xt,xt)*dot_product(zt,zt))
end function
pure function rms(x) result(y)
real(dp), intent(in) :: x(:)
real(dp) :: y
y=sqrt(mean(x**2))
end function
end module

program pointer_metrics
use pointer_metrics_mod
implicit none
real(dp), allocatable, target :: history(:,:), forward(:,:)
real(dp), pointer :: p(:),q(:)
real(dp) :: metrics(1,5)
integer :: column,first,last
allocate(history(3,2),forward(3,2))
history=reshape([1._dp,1._dp,1._dp,1._dp,2._dp,4._dp],[3,2])
forward=reshape([1._dp,1._dp,1._dp,1._dp,1._dp,2._dp],[3,2])
column=2
first=1
last=3
p=>history(first:last,column)
q=>forward(first:last,column)
metrics(1,:)=[rms(log(p)-log(q)),correl(p,q),correl(log(p),log(q)), &
              mean(qlike(q**2,p**2)),0.001_dp*mean(mse_var(q**2,p**2))]
print '(5f12.6)',metrics(1,:)
end program
