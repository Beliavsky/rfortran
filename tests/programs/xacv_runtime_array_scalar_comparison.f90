! Regression for xmask = xindep <= thresh(1) in xacv.f90:21187.
! Expected output: T T F
program test_runtime_array_scalar_comparison
implicit none
double precision,allocatable::xindep(:),thresh(:)
integer::n
n=3
allocate(xindep(n),thresh(1))
xindep=[1d0,2d0,3d0]
thresh=[2d0]
call compare(xindep,thresh)
contains
subroutine compare(xindep,thresh)
double precision,intent(in)::xindep(:),thresh(:)
logical::xmask(size(xindep))
xmask=xindep<=thresh(1)
print *,xmask
end subroutine
end program
