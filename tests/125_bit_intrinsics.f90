program bits
implicit none
integer :: i,j,p,n,result
integer(1) :: small
integer(8) :: wide
integer,parameter :: field=ibits(170,1,3), cleared=ibclr(15,3)
i=10
j=12
p=3
n=4
print *,iand(i,j),ior(i,j),ieor(i,j),not(i)
print *,btest(i,p),ibset(i,0),ibclr(i,p),ibits(i,0,n)
print *,field,cleared
result=ibset(i,p)
if (btest(result,p)) print *,result
small=-1_1
wide=-1_8
print *,kind(ibclr(small,p)),kind(ibits(wide,p,n))
print *,ibclr(small,7),ibset(0_1,7),not(small)
print *,ishft(small,-1),ishft(small,8),ishft(small,-8)
print *,ibits(wide,0,64),ibits(wide,64,0),btest(wide,63)
print *,ishft(wide,-1),ishft(wide,64),ishft(wide,-64)
print *,ibits(pos=1,i=i,len=3),iand(j=j,i=i),ishft(shift=-1,i=i)
print *,checked(i,p)
contains
pure logical function checked(value,pos)
integer,intent(in)::value,pos
checked=btest(value,pos)
end function checked
end program bits
