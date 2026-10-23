! Related example for xacv.f90 line 6550: nested diagonal functions with FORALL.
module diagonals_mod
implicit none
private
public::diag_3d
integer,parameter::dp=kind(1d0)
contains
pure function diag(xx) result(xdiag)
real(kind=dp),intent(in)::xx(:,:)
real(kind=dp)::xdiag(size(xx,1))
integer::i,n1,n2
n1=size(xx,1)
n2=size(xx,2)
forall(i=1:min(n1,n2)) xdiag(i)=xx(i,i)
end function
pure function diag_3d(xx) result(xdiag)
real(kind=dp),intent(in)::xx(:,:,:)
real(kind=dp)::xdiag(size(xx,1)*size(xx,3))
integer::i3,n1,n3,j
n1=size(xx,1)
n3=size(xx,3)
do i3=1,n3
   j=1+(i3-1)*n1
   xdiag(j:j+n1-1)=diag(xx(:,:,i3))
end do
end function
end module
program test
use diagonals_mod
implicit none
real(kind=kind(1d0)),allocatable::x(:,:,:)
integer::n,i,j,k
read *,n
allocate(x(2,2,n))
do k=1,n
do j=1,2
do i=1,2
   x(i,j,k)=i+2*(j-1)+4*(k-1)
end do
end do
end do
print *,diag_3d(x)
! Input: 2. Expected: 1 4 5 8
end program
