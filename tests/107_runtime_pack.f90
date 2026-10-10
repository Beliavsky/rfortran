program runtime_packing
implicit none
integer::a(2,3),v(3),threshold
logical::mask(2,3),flags(4),choose(4)
a=reshape([1,2,3,4,5,6],[2,3])
mask=mod(a,2)==0
print *,pack(a,mask)
print *,10,pack(a,mask)+1,20
v=pack(a,mask)
print *,v
print *,all(pack(a,mask)==[2,4,6])
print *,all([2,4,6]==pack(a,mask))
print *,all(pack(a,mask)==pack(a,a>3)-2)
do threshold=0,6,2
 print *,threshold,sum(pack(a,a>threshold)),product(pack(a,a>threshold))
end do
mask=.false.
print *,pack(a,mask)
print *,sum(pack(a,mask)),all(pack(a,mask)>0),any(pack(a,mask)>0)
flags=[.true.,.false.,.true.,.false.]
choose=[.true.,.true.,.false.,.false.]
print *,pack(flags,choose),count(pack(flags,choose))
print *,minval(pack(a,a>3)),maxval(pack(a,a>3))
print *,f()
contains
pure integer function f()
integer::x(3)
x=[1,2,3]
f=sum(pack(x,x>1))
end function f
end program runtime_packing
