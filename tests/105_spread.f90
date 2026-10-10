program replicate
implicit none
integer::a(2,2),b(2,3,2),empty(0,2)
integer::v(3),scalar
logical::flags(2)
double precision::d(2),x
scalar=7
print *,spread(scalar,1,3)
v=[1,2,3]
print *,spread(v,1,2)
print *,spread(ncopies=2,source=v,dim=2)
a=reshape([1,2,3,4],[2,2])
print *,spread(a,1,2)
b=spread(a,2,3)
print *,b
print *,spread(a,3,2)
print *,spread(v(3:1:-1),2,2)
print *,spread(spread(5,1,2),2,3)
print *,sum(spread(v,1,0)),sum(spread(v,2,-2)),sum(spread(empty,2,3))
flags=[.true.,.false.]
print *,spread(flags,1,2)
d=[1.0d0,2.0d0]
x=3.0d0
print *,spread(d,2,2),spread(x,1,2)
a=spread(a(:,1),2,2)
print *,a,f()
v=[spread(5,1,2),6]
print *,v
contains
pure integer function f()
integer::values(2,3)
values=spread([1,2],2,3)
f=sum(values)
end function f
end program replicate
