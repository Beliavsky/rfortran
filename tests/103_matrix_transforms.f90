program matrix_transforms
implicit none
integer::a(0:1,-1:1),b(3,2),cube(2,3,2),shifts(2,2),empty(0,3)
logical::flags(2,3)
a=reshape([1,2,3,4,5,6],[2,3])
b=transpose(matrix=a)
print *,b
print *,all(transpose(b)==a)
print *,transpose(a(:,0:1))
print *,cshift(a,shift=[1,0,-1],dim=1)
print *,cshift(a,shift=[1,-1],dim=2)
cube=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[2,3,2])
shifts=reshape([1,-1,2,0],[2,2])
print *,cshift(cube,shifts,dim=2)
! SHIFT and ARRAY may overlap the destination.
a=cshift(a,a(:,0),dim=2)
print *,a
flags=mod(a,2)==0
print *,transpose(flags)
print *,sum(transpose(empty)),sum(cshift(empty,[1,2,3],dim=1))
print *,f()
contains
pure integer function f()
integer::x(2,2)
x=reshape([1,2,3,4],[2,2])
x=transpose(x)
x=cshift(x,[1,-1],1)
f=x(1,1)
end function f
end program matrix_transforms
