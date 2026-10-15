program test_runtime_forall
implicit none
integer :: a(4), i, n
real :: matrix(2,2)
a=[1,2,3,4]
n=4
i=99
forall(i=1:n) a(i)=a(n+1-i)
print *,a,i
matrix=reshape([1.0,2.0,3.0,4.0],[2,2])
print *,diagonal(matrix)
contains
pure function diagonal(x) result(y)
real,intent(in)::x(:,:)
real::y(size(x,1))
integer::j,n1,n2
n1=size(x,1)
n2=size(x,2)
forall(j=1:min(n1,n2)) y(j)=x(j,j)
end function diagonal
end program test_runtime_forall
