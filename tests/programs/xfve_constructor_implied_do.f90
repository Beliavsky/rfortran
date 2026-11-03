! Reduced from vol_pred.f90 in C:\fortran\2015\fve_files.txt.
! Expected output: 2.0 3.0 4.0
! The saved/allocated object triggers early constant folding. Before the
! scope fix, removing it made rfort accept the otherwise unchanged implied-DO.
module fve_repro_mod
implicit none
integer, parameter :: nmetrics = 3
type wrapper
   real :: value
end type
contains
subroutine calculate(n)
integer, intent(in) :: n
integer :: ifwd, ilambda, imetric
real, allocatable :: metrics(:,:), metrics_mean(:,:)
type(wrapper), allocatable, target :: object
type(wrapper), save :: saved = wrapper(0.)
allocate(object)
object%value = saved%value
allocate(metrics(n,nmetrics),metrics_mean(1,nmetrics))
metrics(:,1) = [1.0,3.0]
metrics(:,2) = [2.0,4.0]
metrics(:,3) = [3.0,5.0]
loop_ifwd: do ifwd=1,1
   do ilambda=1,1
      metrics_mean(ilambda,:) = [(mean(metrics(:,imetric)), imetric=1,nmetrics)]
   end do
end do loop_ifwd
print '(3f6.1)', metrics_mean(1,:)
end subroutine
function mean(x) result(y)
real, intent(in) :: x(:)
real :: y
y = sum(x)/real(size(x))
end function
end module
program fve_constructor_implied_do
use fve_repro_mod
call calculate(2)
end program
