program reshape_constructors
implicit none
integer::a(2,3),v(6),b(3,2),empty(0,3)
logical::mask(2,3),flags(2,2)
real::r(2,2)
a=reshape(shape=[2,3],source=[1,2,3,4,5,6])
mask=mod(a,2)==0
print *,all(sum(a,dim=1)==[3,7,11]),all(sum(a,dim=2)==[9,12])
print *,all(sum(a,dim=1,mask=mask)==[2,4,6])
print *,all(product(a,dim=2,mask=mask)==[1,48])
print *,all(count(mask,dim=2)==[0,3])
print *,all(minval(a,dim=1,mask=mask)==[2,4,6])
print *,all(maxval(a,dim=2)==[5,6])
v=[a(:,1),99,a(:,3),-1]
print *,v
v=[v(6:1:-1)]
print *,v
b=reshape(a,[3,2])
print *,b,sum(reshape([10,20,30,40],[2,2]),dim=1)
print *,lbound(reshape(v,[2,3])),ubound(reshape(v,[2,3]))
empty=reshape(v,[0,3])
print *,sum(empty),all(sum(empty,dim=1)==[0,0,0])
flags=reshape([.true.,.false.,.true.,.true.],[2,2])
print *,count(flags,dim=1)
r=reshape([1.5,2.5,3.5,4.5],[2,2])
print *,sum(r,dim=1)
print *,mod([-7,7,-8],3),modulo([-7,7,-8],3)
print *,mod([-7.5,7.5],2.0),modulo([-7.5,7.5],2.0)
print *,sum([v(1:2),10]),all([.true.,.true.])
end program reshape_constructors
