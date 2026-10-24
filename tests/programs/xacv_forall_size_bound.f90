! Regression for the SIZE(xx,2) FORALL bound in xacv.f90's acf_mat.
! Expected output: 1 2 3 4 5 6
module forall_size_bound_mod
implicit none
contains
pure function column_prefixes(xx,nrows) result(yy)
double precision,intent(in)::xx(:,:)
integer,intent(in)::nrows
double precision::yy(nrows,size(xx,2))
integer::icol
forall(icol=1:size(xx,2)) yy(:,icol)=prefix(xx(:,icol),nrows)
end function

pure function prefix(xx,nrows) result(yy)
double precision,intent(in)::xx(:)
integer,intent(in)::nrows
double precision::yy(nrows)
yy=xx(:nrows)
end function
end module

program test_forall_size_bound
use forall_size_bound_mod
implicit none
double precision,allocatable::xx(:,:)
integer::n
n=2
allocate(xx(n,3))
xx=reshape([1d0,2d0,3d0,4d0,5d0,6d0],[2,3])
print *,column_prefixes(xx,n)
end program
