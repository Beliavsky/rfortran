! Fortran 2023: type parameters are copied; rank and bounds are independent.
use iso_fortran_env, only: real64
implicit none
real(real64) :: x(3)
typeof(x) :: total, y(2)
character(5) :: label
typeof(label) :: copy
x = [1.0_real64, 2.0_real64, 3.0_real64]
total = sum(x)
y = [total, total]
label = 'ready'
copy = label
print *, size(y), kind(total) == real64, len(copy)
print '(a)', copy
end
