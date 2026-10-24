! Regression for xacv.f90:25856: mean(ret_use**2).
! Expected output: 6.500, the mean of 2**2 and 3**2.
! A fixed TARGET avoids the separate ALLOCATABLE-target association limitation.
module pointer_mean_mod
implicit none
integer,parameter::dp=kind(1d0)
contains
pure function mean(xx) result(xmean)
real(dp),intent(in)::xx(:)
real(dp)::xmean
integer::n
n=size(xx)
if(n>0) then
   xmean=sum(xx)/n
else
   xmean=0.0_dp
end if
end function
end module

program test_pointer_expression_actual
use pointer_mean_mod
implicit none
real(dp),target::zret(3)
real(dp),pointer::ret_use(:)
integer::i1
zret=[1.0_dp,2.0_dp,3.0_dp]
i1=2
ret_use=>zret(i1:)
print '(f0.3)',mean(ret_use**2)
end program
