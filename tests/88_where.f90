program masked_assignment
implicit none
integer::a(5),b(5),c(5),where(1),elsewhere,endwhere
logical::m(5),n(5),empty(0)
real::r(5)
! New keywords remain usable as names.
where(1)=2
elsewhere=3
endwhere=4
print *,where(1)+elsewhere+endwhere
a=[1,2,3,4,5]
b=0
m=[.true.,.false.,.true.,.false.,.true.]
n=.not.m
print *,m
print *,n
print *,all(m .or. n),any(m .and. n)
WHERE (m) b=a
print *,b
! Capture mask before A changes; subsequent RHS sees the updated A.
where(a<=3)
 a=a+10
 b=a
else where
 b=-a
end where
print *,a
print *,b
! Snapshot selected RHS elements before writing an overlapping destination.
a=[1,2,3,4,5]
where(m) a=a(5:1:-1)
print *,a
! Inactive RHS elements may be undefined or divide by zero.
c(1)=10
c(3)=30
c(5)=50
where(m) b=c
print *,b
r=0.0
where(a/=3) r=6.0/(a-3)
print *,r
! Earlier branches take precedence, even after changing mask inputs.
a=[1,2,3,4,5]
where(a<=2)
 a=100
elsewhere(a<=4)
 a=200
elsewhere
 a=300
endwhere
print *,a
! LOGICAL destination and mask may be the same array.
where(m)
 m=.false.
elsewhere
 m=.true.
end where
print *,m
where(empty) empty=.true.
print *,all(empty),any(empty)
! A reduction RHS must see the entire source, not only active elements.
where(m) b=sum(a)
print *,b
! Array-valued ELEMENTAL calls must evaluate only active elements.
a=[1,2,3,4,5]
where(a/=3) b=divide(a)
print *,b
m(:)=.true.
m(2:4:2)=.false.
print *,m
contains
elemental integer function divide(x) result(y)
integer,intent(in)::x
y=12/(x-3)
end function divide
end program masked_assignment
