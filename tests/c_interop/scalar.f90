! Scalar BIND(C) exports; run with caller.c.
function plus(i,x) bind(c,name="AddMixed") result(r)
use iso_c_binding,only:c_int,c_float
implicit none
integer(c_int),value :: i
real(c_float),value :: x
integer(c_int) :: r
i=i+1
r=i+int(x)
end function

subroutine bump(x,y) bind(c,name="bump_values")
use iso_c_binding
implicit none
integer(c_int),intent(inout)::x
real(c_double),intent(out)::y
x=x+2
y=3.5_c_double
end subroutine
