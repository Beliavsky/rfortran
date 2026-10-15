program complex_demo
use, intrinsic :: iso_fortran_env, only: real64
implicit none
complex(real64) :: z, w, values(3)
z = (3.0_real64, 4.0_real64)
w = conjg(z)
values = [z, w, z*w]
print *, 'z = ', z
print *, 'conjg(z) = ', w
print *, 'abs(z) = ', abs(z)
print *, 'values = ', values
print *, 'dot_product(values,values) = ', dot_product(values,values)
end program complex_demo
