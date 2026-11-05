program demo
implicit none
integer :: n
real, allocatable :: x(:),y(:)
n=10000
allocate(x(n),y(n))
x=4.0
call transform(x,y)
print *,y(1),y(n)
contains
subroutine transform(x,y)
real,intent(in) :: x(:)
real,intent(out) :: y(:)
integer :: i
do concurrent(i=1:size(x)) shared(x,y) default(none)
   y(i)=sqrt(x(i))
end do
end subroutine
end program
