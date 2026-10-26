program main
implicit none
integer, parameter :: n=1000000
real(kind=8) :: x(n,3)
x(:,1)=1.0_8
x(:,2)=2.0_8
x(:,3)=3.0_8
x(:,3)=x(:,3)+x(:,1)+2*x(:,2)
print *,sum(x(:,3)),x(1,3),x(n,3)
end
