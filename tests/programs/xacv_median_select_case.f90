! Repro for the generated-storage diagnostic at xacv.f90:20064.
! Expected output: 3.00. Only the median's zero/one/two-item path is retained.
module median_repro_mod
implicit none
integer,parameter::dp=kind(1.0d0)
contains
function stat_vec(label,xx) result(xstat)
character(*),intent(in)::label
real(dp),intent(in)::xx(:)
real(dp)::xstat
select case(label)
case('median')
   xstat=median(xx)
case default
   xstat=sum(xx)
end select
end function
function median(xx) result(xmed)
real(dp),intent(in)::xx(:)
real(dp)::xmed,xcopy(size(xx))
xcopy=xx
call median_sub(xcopy,size(xx),xmed)
end function
subroutine median_sub(x,n,xmed)
integer,intent(in)::n
real(dp),intent(inout)::x(:)
real(dp),intent(out)::xmed
if(n<1)then
   xmed=0.0_dp
   return
end if
xmed=x(1)
if(n==1)return
xmed=0.5_dp*(xmed+x(2))
end subroutine
end module
program main
use median_repro_mod
implicit none
print '(f6.2)',stat_vec('median',[2.0_dp,4.0_dp])
end program
