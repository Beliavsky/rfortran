program numeric_model
!#allow(unused_program_var)
implicit none
integer(1) :: small
integer(2) :: medium
integer(8) :: large
real :: x, a(3)
double precision :: y, matrix(2,2)
integer, parameter :: mantissa = digits(1.0_8)
real, parameter :: eps = epsilon(1.0)
double precision, parameter :: deps = epsilon(1.0_8)
print *, digits(small),digits(medium),digits(large)
print *, range(small),range(medium),range(large)
print *, digits(x),digits(y),mantissa
print *, precision(x),precision(y),range(x),range(y)
print *, minexponent(x),minexponent(y),maxexponent(x),maxexponent(y)
print *, radix(x),radix(large),radix(a),radix(matrix)
print *, kind(epsilon(x)),kind(epsilon(y))
print *, epsilon(x)/eps,epsilon(y)/deps
print *
print *, digits(x+y),range(large+small),precision(x**large)
print *, digits(x=matrix),epsilon(x=y)/deps
end program numeric_model
