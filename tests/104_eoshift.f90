program end_off_shifts
implicit none
integer::a(2,3),v(4),cube(2,2,2),empty(0,2)
logical::flags(3)
double precision::d(3),boundary
v=[1,2,3,4]
print *,eoshift(v,1),eoshift(v,-2,9),eoshift(v,0)
a=reshape([1,2,3,4,5,6],[2,3])
print *,eoshift(a,shift=[1,-1],boundary=[99,88],dim=2)
print *,eoshift(dim=1,boundary=[7,8,9],shift=1,array=a)
! Both boundary and shift may alias the destination.
a=eoshift(a,a(:,1),a(:,3),2)
print *,a
cube=reshape([1,2,3,4,5,6,7,8],[2,2,2])
print *,eoshift(cube,reshape([1,-1,0,3],[2,2]),dim=2)
flags=[.true.,.true.,.true.]
print *,eoshift(flags,-1),eoshift(flags,3,.true.)
d=[1.0d0,2.0d0,3.0d0]
boundary=4.0d0
print *,eoshift(d,1),eoshift(d,-2,boundary)
print *,sum(eoshift(empty,1)),sum(eoshift(empty,[1,2],dim=1))
v=eoshift(v(4:1:-1),-1)
print *,v,f()
contains
pure integer function f()
integer::x(3)
x=[1,2,3]
x=eoshift(x,-1,9)
f=sum(x)
end function f
end program end_off_shifts
