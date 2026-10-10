program masked
integer :: a(-2:2), e(0), m(2,2)
logical :: mask(5)
real :: r(5)
a=[1,2,3,4,5]
mask=a>2
r=[1.0,2.0,3.0,4.0,5.0]
m=2
print *,sum(array=a),sum(mask=mask,array=a),product(a,mask=mask)
print *,minval(array=a,mask=mask),maxval(mask=mask,array=a)
print *,sum(a,dim=1),product(mask=mask,dim=1,array=a)
print *,sum(a,mask=.false.),product(a,mask=.false.)
print *,sum(e,dim=1),product(e,dim=1)
print *,minval(e),maxval(e)
print *,minval(a,mask=.false.),maxval(a,mask=.false.)
print *,sum(a(-1:2:2),mask=a(-1:2:2)>2)
print *,sum(m,mask=m>1)
print *,sum(r,mask=mask),minval(r,mask=mask),maxval(r,mask=mask)
end
