program empty_matrices
implicit none
integer :: a(0,3), b(2,0), c(2,0,3), z(2:1,-2:0)
logical :: m(2,3), e(0,3)
real :: r(0,2)
a=1
b=2
c=3
z=4
r=1.0
e=.true.
m=.true.
m(2,2)=.false.
print *,size(a),size(a,1),size(a,2),lbound(a),ubound(a)
print *,size(b),size(b,1),size(b,2),lbound(b),ubound(b)
print *,size(c),lbound(c),ubound(c)
print *,lbound(z),ubound(z),size(z)
print *,sum(a),product(b),minval(c),maxval(z)
print *,sum(a(:,:)),product(b(:,:)),sum(r)
print *,count(e),any(e),all(e),count(e(:,:))
print *,count(m),any(m),all(m)
print *,m(:,2),m(2,:)
m(:,3)=m(:,2)
print *,count(m),m
print *,sum(a,mask=e),product(a,mask=.false.)
end program empty_matrices
