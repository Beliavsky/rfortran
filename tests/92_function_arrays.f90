program function_arrays
implicit none
integer :: a(-2:2), b(5), matrix(0:1,-1:0), e(0), i
real :: r(3)
real(kind=8) :: d(3)
logical :: mask(3)
a=[1,2,3,4,5]
b=[5,4,3,2,1]
matrix(0,-1)=1
matrix(1,-1)=2
matrix(0,0)=3
matrix(1,0)=4
r=[1.0,2.0,3.0]
d=[1.0d0,2.0d0,3.0d0]
mask=[.true.,.false.,.true.]
print *,array_sum(a),array_sum(b),array_sum(a)+array_sum(b)
print *,pair_sum(a,2,b),positive(a),bounded(a)
print *,matrix_sum(matrix),empty_sum(e)
print *,real_sum(r),double_sum(d),logical_count(mask)
i=0
do while (array_sum(a)>12)
 i=i+1
 a(-2)=a(-2)-1
end do
print *,i,array_sum(a),relay(b)
contains
pure integer function array_sum(x) result(n)
integer(kind=4),intent(in) :: x(5)
n=sum(x)
end function array_sum
pure integer function pair_sum(x,k,y) result(n)
integer,intent(in) :: x(5),k,y(5)
n=sum(x)+k*sum(y)
end function pair_sum
pure integer function positive(x) result(n)
integer,intent(in) :: x(5)
n=count(x>0)
end function positive
pure integer function bounded(x) result(n)
integer,intent(in) :: x(20:24)
n=x(20)+x(24)+lbound(x,1)
end function bounded
pure integer function matrix_sum(x) result(n)
integer,intent(in) :: x(2,2)
n=x(1,1)+10*x(2,1)+100*x(1,2)+1000*x(2,2)
end function matrix_sum
pure integer function empty_sum(x) result(n)
integer,intent(in) :: x(0)
n=sum(x)
end function empty_sum
pure real function real_sum(x) result(n)
real,intent(in) :: x(3)
n=sum(x)
end function real_sum
pure function double_sum(x) result(n)
real(kind=8),intent(in) :: x(3)
real(kind=8) :: n
n=sum(x)
end function double_sum
pure integer function logical_count(x) result(n)
logical,intent(in) :: x(3)
n=count(x)
end function logical_count
pure integer function relay(x) result(n)
integer,intent(in) :: x(5)
n=array_sum(x)+array_sum(x)
end function relay
end program function_arrays
