program nested_masks
implicit none
integer::a(2,3),b(2,3),c(2,3),empty(0,2)
logical::mask(2,3)
a=reshape([1,2,3,4,5,6],[2,3])
b=-1
mask=mod(a,2)==0
where(mask)
 where(a>2)
  where(a<6)
   b=100+a
  elsewhere
   b=200+a
  end where
 elsewhere
  b=300+a
 end where
elsewhere(a<4)
 b=-a
elsewhere
 b=0
end where
print *,b
! Both masks must retain their selected elements after a changes.
where(mask)
 where(a>2)
  a=-a
  b=a
 elsewhere
  b=20
 end where
 b=b+1
end where
print *,a,b
! Only selected mask elements may be read.
c(2,1)=7
c(2,2)=8
c(2,3)=9
where(mask)
 where(c>7)
  b=c
 elsewhere(c==7)
  b=70
 elsewhere
  b=90
 end where
end where
print *,b
! Empty nested masks must execute no element operations.
where(empty>0)
 where(empty<3)
  empty=empty+1
 elsewhere
  empty=0
 end where
end where
print *,sum(empty)
end program nested_masks
