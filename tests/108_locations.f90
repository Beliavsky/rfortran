program locations
implicit none
integer::a(0:1,-1:1),cube(2,2,2),v(4),empty(0,3)
integer(8)::wide(2)
logical::back,flags(4)
real::r(4)
a=reshape([4,1,1,7,1,7],[2,3])
back=.true.
print *,minloc(a),maxloc(a),minloc(a,back=back),maxloc(a,back=back)
print *,minloc(a,dim=1),maxloc(a,dim=2),maxloc(a,dim=2,back=back)
print *,minloc(a,dim=2,mask=a==1),maxloc(a,mask=.false.)
print *,findloc(a,7),findloc(a,7,back=back),findloc(a,7,dim=2,back=back)
print *,findloc(a,99),findloc(a,7,dim=1,mask=.false.)
wide=minloc(a,kind=8)
print *,wide,kind(wide)
v=[3,1,1,4]
print *,minloc(v,1),minloc(v,1,back=back),findloc(v,1,1,back=back)
print *,minloc(v(4:1:-1)),maxloc(a(:,0:1))
cube=reshape([5,1,1,4,3,3,7,2],[2,2,2])
print *,minloc(cube,dim=2),maxloc(cube,dim=3),findloc(cube,3,dim=1,back=back)
print *,minloc(empty),maxloc(empty,dim=1),findloc(empty,1,dim=2)
flags=[.true.,.false.,.false.,.true.]
print *,findloc(flags,.false.),findloc(flags,.false.,back=back)
r=[2.5,-1.0,-1.0,4.0]
print *,minloc(r,back=back),maxloc(r),findloc(r,-1.0,back=back)
print *,f()
contains
pure integer function f()
integer::x(3)
x=[1,2,2]
f=maxloc(x,dim=1,back=.true.)
end function f
end program locations
