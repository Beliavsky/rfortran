! Fixed storage with runtime selectors and bounds after function specialization.
program test_fixed_runtime_sections
implicit none
real(8)::a(2,2)
integer::i,j
a=matrix([1.d0,2.d0,3.d0,4.d0],2)
do j=1,2
   print *,a(:,j)
end do
i=2
a(:,i)=a(2:1:-1,1)
print *,a
contains
function matrix(x,ncol) result(y)
real(8),intent(in)::x(:)
integer,intent(in)::ncol
real(8)::y(size(x)/ncol,ncol)
integer::col,nrows
nrows=size(x)/ncol
forall(col=1:ncol) y(:,col)=x(1+(col-1)*nrows:col*nrows)
end function
end program
