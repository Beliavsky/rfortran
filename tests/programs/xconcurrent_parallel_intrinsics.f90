! Scalar intrinsics in direct loops and PURE/SIMPLE callees.
implicit none
integer :: i, values(9000), rounded
real :: x
do concurrent (i=1:9000) local(x,rounded)
   x=real(modulo(i,100))/100.0
   rounded=floor(x)+ceiling(x)
   values(i)=rounded+nint(tan(atan(x))-x)
   call adjust(values(i))
end do
if (values(100)/=2 .or. values(1)/=3) stop 1
print *, 'PASS'
contains
simple subroutine adjust(value)
   integer, intent(inout) :: value
   value=value+offset(real(value))
end subroutine
simple integer function offset(x) result(n)
   real, intent(in) :: x
   n=nint(aint(x)+anint(x))-nint(2.0*x)+digits(x)-22
end function
end
