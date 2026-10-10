module elemental_math
implicit none
contains
elemental function sech(x) result(y)
real(8), intent(in) :: x
real(8) :: y
y=1.0d0/cosh(x)
end function sech
pure elemental integer function scaled(x,factor)
integer, intent(in) :: x, factor
scaled=x*factor
end function scaled
elemental pure function plus_one(x) result(y)
integer, intent(in) :: x
integer :: y
y=x+1
end function plus_one
end module elemental_math
program elemental_test
use elemental_math, only: sech, scaled, plus_one
implicit none
integer :: a(3), b(3), m(2,2), elemental
real(8) :: x(3), y(3)
elemental=7
a=[1,2,3]
b=scaled(a,2)
print *, b, elemental
print *, scaled(3,4)
print *, scaled(plus_one(a),b)
a=scaled(a,3)
print *, a
print *, sum(scaled(a(1:3:2),2))
print *, sum(scaled(a(3:2),2))
m=4
m=scaled(m,2)
print *, m
print *, scaled(m(:,2),3)
x=[0.0d0,1.0d0,-1.0d0]
y=sech(x)
print *, y, sech(0.0d0)
end program elemental_test
