! Regression for xacv.f90:25198, the nested corr_mat(xx) argument.
! Expected output: 2 2 4 (result shape and sum).
! No PACK call is needed. The generated @optional_N number may vary.
! Passing the result directly must work as passing a stored local array does.
module matrix_result_mod
implicit none
contains
function make_matrix(xx,good) result(yy)
double precision,intent(in)::xx(:,:)
logical,intent(in),optional::good(:,:)
double precision::yy(size(xx,2),size(xx,2))
yy=1d0
if(present(good)) yy=0d0
end function

subroutine consume(yy)
double precision,intent(in)::yy(:,:)
print *,shape(yy),int(sum(yy))
end subroutine

subroutine outer(xx)
double precision,intent(in)::xx(:,:)
call consume(make_matrix(xx))
end subroutine
end module

program test_nested_runtime_array_function
use matrix_result_mod
implicit none
double precision,allocatable::xx(:,:)
integer::n
n=3
allocate(xx(n,2))
xx=1d0
call outer(xx)
end program
