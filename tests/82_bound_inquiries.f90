program bound_inquiries
implicit none
integer :: a(2,3), z(0), lo(2), hi(2)
print *, lbound(a,1), ubound(a,dim=2)
print *, lbound(array=a), ubound(array=a)
lo=lbound(a)
hi=ubound(a)
print *, lo,hi
print *, lbound(a(:,2:3)),ubound(a(:,2:3))
print *, lbound(a(2:1:-1,3:1:-2),dim=2),ubound(a(2:1:-1,3:1:-2),dim=2)
print *, lbound(a(1,:)),ubound(a(1,:))
print *, lbound(z),ubound(z),ubound(z,1)
print *, lbound(a(2:1,:)),ubound(a(2:1,:))
print *, ubound(kind=4,dim=2,array=a),size(dim=2,array=a)
print *, sum(ubound(a))
call show(a)
print '(i0)',width()
contains
subroutine show(x)
integer, intent(in) :: x(:,:)
print *, lbound(x,dim=1),ubound(x,dim=2)
print *, ubound(x)
end subroutine show
pure integer function width() result(n)
integer :: values(3)
values=2
n=ubound(values,dim=1)+values(1)
end function width
end program bound_inquiries
