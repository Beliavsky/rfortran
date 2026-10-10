program section_assignment
implicit none
integer :: a(3,3), v(5), i,j, t(2,2,2)
real(8) :: x(3,2), y(3,2), factors(2)
do j=1,3
do i=1,3
a(i,j)=10*j+i
end do
end do
a(:,2:3)=a(:,1:2)
print *, a
a(3:1:-1,:)=a
print *, a
a(1:3:2,1:3:2)=-1
print *, a
a(:,2)=a(:,1)+a(:,3)
print *, a(:,2)
v(:)=[1,2,3,4,5]
v(2:5)=v(1:4)
print *, v
v(5:1:-1)=v
print *, v
v(3:2)=v(2:1)
print *, v
t=0
t(:,2,:)=7
print *, t
x=8.0d0
factors=[2.0d0,4.0d0]
call scale_columns(x,factors,y)
print *, y
v(:)=once()
print *, v
contains
integer function once()
print *, 777
once=42
end function once
subroutine scale_columns(xx,factor,yy)
real(8),intent(in)::xx(:,:),factor(:)
real(8),intent(out)::yy(:,:)
yy(:,1)=xx(:,1)/factor(1)
yy(:,2)=xx(:,2)/factor(2)
end subroutine scale_columns
end program section_assignment
