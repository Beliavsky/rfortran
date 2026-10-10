program dim_reductions
integer :: a(0:1,-1:1),empty(0,3), r(3), s(2)
logical :: mask(2,3),lempty(0,3)
a(0,-1)=1
a(1,-1)=2
a(0,0)=3
a(1,0)=4
a(0,1)=5
a(1,1)=6
mask=a>3
r=sum(a,dim=1)
s=sum(a,dim=2)
print *,r,s
print *,sum(a,dim=1,mask=mask),sum(a,dim=2,mask=mask)
print *,product(a,dim=1),product(a,dim=2,mask=mask)
print *,count(mask,dim=1),count(mask,dim=2,kind=8)
print *,any(mask,dim=1),all(mask,dim=2)
print *,minval(a,dim=1),maxval(a,dim=2)
print *,sum(sum(a,dim=1)),sum(sum(a,dim=2))
print *,lbound(sum(a,dim=1)),ubound(sum(a,dim=2))
print *,sum(empty,dim=1),product(empty,dim=1)
print *,minval(empty,dim=1),maxval(empty,dim=1)
print *,sum(sum(empty,dim=2)),all(sum(empty,dim=1)==0)
print *,minval(a,dim=1,mask=.false.),maxval(a,dim=2,mask=.false.)
print *,count(lempty,dim=1),any(lempty,dim=1),all(lempty,dim=1)
print *,sum(count(lempty,dim=2)),all(all(lempty,dim=2))
end program dim_reductions
