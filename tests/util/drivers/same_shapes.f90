! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_same_shapes
use util_mod, only:same_shape
use kind_mod, only:dp
implicit none
real(dp)::a(2),b(3),c(2,2),d(2,3),e(1,2,3),f(1,3,2)
a=1; b=1; c=1; d=1; e=1; f=1
print *,same_shape(a,a),same_shape(a,b)
print *,same_shape(c,c),same_shape(c,d)
print *,same_shape(e,e),same_shape(e,f)
end program test_same_shapes
