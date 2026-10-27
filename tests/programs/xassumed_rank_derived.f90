program p
implicit none
type point
integer :: x
real :: y(2)
end type
type(point) :: a,b(3),c(2,2)
a%x=1
a%y=[2.,3.]
b%x=4
c%x=5
call change(a)
call change(b)
call change(c)
print *,a%x,a%y,b%x,c%x
contains
subroutine change(x)
type(point),intent(inout)::x(..)
select rank(v=>x)
rank(0)
v%x=v%x+10
v%y=v%y+1
rank(1)
v%x=v%x+20
rank(2)
v%x=v%x+30
end select
end subroutine
end program
