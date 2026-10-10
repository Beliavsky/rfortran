program packing
implicit none
integer::a(2,3),v(5),field(2,3),empty(0)
logical::mask(2,3),flags(4)
double precision::d(3),tail(4)
a=reshape([1,2,3,4,5,6],[2,3])
mask=mod(a,2)==0
v=[10,20,30,40,50]
print *,pack(a,mask=mask,vector=v)
print *,pack(a,.true.)
print *,pack([1,2,3,4],[.false.,.true.,.true.,.false.])
print *,sum(pack(a,.false.)),sum(pack(empty,.true.))
print *,unpack([7,8,9],mask=mask,field=-a)
field=unpack(vector=v,field=99,mask=mask)
print *,field
v=pack(v,v>20,vector=v)
print *,v
flags=[.true.,.false.,.true.,.false.]
print *,pack(flags,flags,vector=flags)
print *,unpack([.false.,.true.],flags,.true.)
d=[1.0d0,2.0d0,3.0d0]
tail=9.0d0
print *,pack(d,d>1,tail)
print *,unpack(d,[.true.,.false.,.true.],d)
print *,f()
contains
pure integer function f()
integer::x(3)
x=unpack([1,2],[.true.,.false.,.true.],9)
f=sum(pack(x,x>1,vector=x))
end function f
end program packing
