program names
use, intrinsic :: iso_fortran_env, only: real64
implicit none
real(real64)::double,precision(3)
double precision::d
integer::i
double=0.5_real64
precision=[1.0_real64,2.0_real64,3.0_real64]
d=twice(double)
print *,double,precision,d
print *,kind(double),kind(precision),kind(d)
do i=1,3
precision(i)=twice(precision(i))
end do
print *,precision
contains
pure double precision function twice(double)
double precision,intent(in)::double
twice=2.0d0*double
end function twice
end program names
