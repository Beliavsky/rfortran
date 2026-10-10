program reshape_options
implicit none
integer::a(2,3),b(2,2,3),source(2),padding(2),empty(0)
logical::flags(2,2)
double precision::d(2,2)
source=[1,2]
padding=[8,9]
a=reshape(source,[2,3],pad=padding)
print *,a
a=reshape(order=[2,1],shape=[2,3],source=source,pad=padding)
print *,a
b=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[2,2,3],order=[3,1,2])
print *,b
! Source and padding may alias the destination.
a=reshape(a,[2,3],order=[2,1])
print *,a
source=reshape(empty,[2],pad=source(2:1:-1))
print *,source
print *,reshape([1,2,3],[5],[7,8],[1])
print *,sum(reshape(empty,[0,3],pad=empty,order=[2,1]))
flags=reshape([.true.],[2,2],pad=[.false.],order=[2,1])
print *,flags
d=reshape([1.0d0],[2,2],pad=[2.0d0,3.0d0],order=[2,1])
print *,sum(d)
end program reshape_options
