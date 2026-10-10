program matrix_sections
implicit none
integer, parameter :: n=3
integer :: a(n,2), b(2,2), v(3), t(2,2,2), i,j
 do j=1,2
 do i=1,n
 a(i,j)=10*j+i
 end do
 end do
b=a(2:,:)-a(:n-1,:)
print *, b
print *, a(:,2)
print *, a(3:1:-1,2:1:-1)
v=a(:,1)+a(:,2)
print *, v
print *, sum(a(1:3:2,:))
print *, sum(a(3:2,:))
call differences(a,b)
print *, b
t=5
t(2,1,2)=9
print *, t(:,1,:)
a=a(3:1:-1,:)
print *, a
contains
subroutine differences(xx,yy)
integer, intent(in) :: xx(:,:)
integer, intent(out) :: yy(:,:)
integer :: rows
rows=size(xx,1)
yy=xx(2:,:)-xx(:rows-1,:)
end subroutine differences
end program matrix_sections
