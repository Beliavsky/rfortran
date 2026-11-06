! All integer kinds and boundary cases are checked by the companion Python suite.
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
 y(i)=probe(i)
end do
if(any(y/=0)) stop 1
print *, 'PASS'
contains
pure integer function probe(i) result(r)
integer,intent(in)::i
integer::n,x
n=modulo(i,33)
x=modulo(i,127)
r=0
if(ishftc(x,bit_size(x))/=x) r=r+1
if(maskr(n)/=ibits(-1,0,n)) r=r+1
if(maskl(n)/=not(ibits(-1,0,32-n))) r=r+1
if(.not.bgt(-1,x)) r=r+1
if(.not.blt(-1_1,-1_8)) r=r+1
if(.not.bge(x,x)) r=r+1
if(.not.ble(x,x)) r=r+1
end function
end
