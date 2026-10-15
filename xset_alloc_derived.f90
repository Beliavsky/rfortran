module derived_concat_mod
implicit none
type :: point
   integer :: id=0
   real :: value=0.0
   logical :: good=.false.
end type point
contains
pure function concat(xx,yy) result(zz)
type(point), intent(in) :: xx(:),yy(:)
type(point) :: zz(size(xx)+size(yy))
integer :: nx
nx=size(xx)
zz(:nx)=xx
zz(nx+1:)=yy
end function concat
pure subroutine set_alloc(xx,yy)
type(point), intent(in) :: xx(:)
type(point), allocatable, intent(out) :: yy(:)
allocate(yy(size(xx)))
yy=xx
end subroutine set_alloc
end module derived_concat_mod

program test_derived_concat
use derived_concat_mod
implicit none
type(point), allocatable :: x(:),out(:)
integer :: i
allocate(x(0))
call set_alloc(concat(x,[point(1,1.5,.true.),point(2,2.5,.false.)]),out)
print *,size(out)
do i=1,size(out)
   print *,out(i)%id,out(i)%value,out(i)%good
end do
deallocate(x)
allocate(x(2))
x=[point(3,3.5,.true.),point(4,4.5,.false.)]
call set_alloc(concat(x,[point(1,1.5,.true.),point(2,2.5,.false.)]),out)
print *,size(out)
do i=1,size(out)
   print *,out(i)%id,out(i)%value,out(i)%good
end do
end program test_derived_concat
