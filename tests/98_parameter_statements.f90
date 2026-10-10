module parameter_values
implicit none
integer::n,m
parameter(n=3,m=n+2)
character(4)::label
parameter(label='ab')
contains
pure integer function local_value() result(v)
integer::n,k
parameter(n=10,k=n+1)
v=k
end function local_value
end module parameter_values
program parameter_statements
use parameter_values, only:n,m,label,local_value
implicit none
integer::i,j,a(m),parameter(2)
real::r
double precision::d
parameter(i=5,j=i+2,r=1.25,d=2.5d0)
a=j
parameter(1)=11
parameter(2)=12
print *,i,j,n,m,sum(a)
print *,r,d,'['//label//']',parameter
print *,local_value()
end program parameter_statements
