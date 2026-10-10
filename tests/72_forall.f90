program forall_test
implicit none
integer :: i, a(5), m(2,3), forall
real(8)::x(2,3),y(2,3),factors(3)
a=[1,2,3,4,5]
i=99
forall (i=1:size(a)) a(i)=a(6-i)
print *,a,i
forall (i=5:1:-2) a(i)=10*i
print *,a
forall (i=2:1) a(i)=0
print *,a
m=0
forall (i=1:3) m(:,i)=i
print *,m
forall (i=1:3) m(:,i)=m(:,4-i)
print *,m
x=12.0d0
factors=[2.0d0,3.0d0,4.0d0]
call scale_columns(x,factors,y)
print *,y
forall=7
print *,forall
forall(i=1:5) a(i)=twice(a(i))
print *,a
print *,checksum()
contains
pure integer function twice(k)
integer,intent(in)::k
twice=2*k
end function twice
pure integer function checksum()
integer::j,v(3)
forall(j=1:3) v(j)=j
checksum=sum(v)
end function checksum
pure subroutine scale_columns(xx,factor,yy)
real(8),intent(in)::xx(:,:),factor(:)
real(8),intent(out)::yy(:,:)
integer::i,n2
n2=size(xx,2)
forall(i=1:n2) yy(:,i)=xx(:,i)/factor(i)
end subroutine scale_columns
end program forall_test
