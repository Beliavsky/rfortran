program shifts
implicit none
integer :: a(6), b(0:1,-1:1), shift
integer :: empty(0,2)
logical :: mask(6)
a=[1,2,3,4,5,6]
shift=-1
mask=[.false.,.true.,.true.,.true.,.true.,.false.]
where(mask)
 a=cshift(a,shift)
end where
print *,a
print *,cshift(a,14),cshift(a,-14)
print *,cshift(array=a(6:2:-2),shift=1)
b=reshape([1,2,3,4,5,6],[2,3])
print *,all(cshift(b,1,dim=1)==reshape([2,1,4,3,6,5],[2,3]))
print *,all(cshift(b,dim=2,shift=-1)==reshape([5,6,1,2,3,4],[2,3]))
print *,all(cshift(mask,1).eqv.[.true.,.true.,.true.,.true.,.false.,.false.])
print *,sum(cshift(empty,1,1)),sum(cshift(empty,-1,2))
print *,sum(cshift([1.0,2.0,3.0],-1))
print *,f()
contains
pure integer function f()
integer::v(3)
v=[1,2,3]
v=cshift(v,1)
f=v(1)
end function f
end program shifts

