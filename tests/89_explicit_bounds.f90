program explicit_bounds
implicit none
integer,parameter::lo=-2,hi=2
integer::a(0:1,-1:1),v(lo:hi),empty(5:3),i,j,w(10:14)
logical::mask(-2:2)
v=[10,20,30,40,50]
mask=[.true.,.false.,.true.,.false.,.true.]
do j=-1,1
 do i=0,1
  a(i,j)=10*j+i
 end do
end do
print *,a
print *,v(lbound(v,1)),v(ubound(v,1)),v(hi:lo:-1),v(::-1)
print *,lbound(a),ubound(a),size(a)
print *,lbound(v),ubound(v),lbound(v(:)),ubound(v(:))
print *,lbound(a,dim=2),ubound(array=a,dim=1)
print *,lbound(empty),ubound(empty),size(empty)
print *,a(:,0),a(1,-1:1),a(1:0:-1,1:-1:-1)
! Section assignments translate subscripts once and preserve RHS snapshots.
v(-1:1)=v(0:2)
print *,v
w=v
print *,w,lbound(w),ubound(w)
i=-2
v(i)=77
print *,v(i),mask(i)
where(mask) v=-v
print *,v
! Default assumed-shape dummy lower bounds are one, regardless of the actual.
call assumed(v)
! An explicit-shape dummy uses its own declared lower bounds.
call explicit(v)
print *,v
! Local declarations shadow the outer array's bounds.
block
 integer::v(4:5)
 v=[7,8]
 print *,v(4),lbound(v),ubound(v)
end block
print *,lbound(v),ubound(v)
contains
subroutine assumed(x)
integer,intent(in)::x(:)
print *,lbound(x),ubound(x),x(1)
end subroutine assumed
subroutine explicit(x)
integer,intent(inout)::x(20:24)
integer::t
t=21
x(t)=99
print *,lbound(x),ubound(x),x(t)
end subroutine explicit
end program explicit_bounds
