! Regression for xacv.f90:25805: a rank-two LOGICAL expression as INTENT(IN) actual.
! Expected output: 2 2 3 (mask extents and number of true elements).
! Former rfort error: type or rank mismatch for `good`.
! Assigning the expression to an allocatable LOGICAL matrix first avoids it.
module stats_mask_mod
implicit none
integer,parameter::dp=kind(1d0)
contains
subroutine show_stats(xx,good)
real(dp),intent(in)::xx(:,:)
logical,intent(in),optional::good(:,:)
if(present(good)) print *,size(xx,1),size(good,2),count(good)
end subroutine
end module

program test_runtime_logical_mask_actual
use stats_mask_mod
implicit none
real(dp),parameter::bad_value=-999.0_dp
real(dp),allocatable::xx(:,:)
integer::n
n=2
allocate(xx(n,2))
xx=1.0_dp
xx(1,1)=bad_value
call show_stats(xx,good=abs(xx-bad_value)>1.0e-10_dp)
end program
