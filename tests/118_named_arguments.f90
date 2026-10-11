module named_arguments_module
implicit none
contains
subroutine sub(x, x2, x3)
real, intent(in) :: x
real, intent(out) :: x2, x3
x2 = 2*x
x3 = 3*x
end subroutine sub
pure integer function encode(a, b, c)
integer, intent(in) :: a, b, c
encode = 100*a + 10*b + c
end function encode
pure integer function combine(a, b, c)
integer, intent(in) :: a, c
integer, intent(in), optional :: b
combine = a + 100*c
if (present(b)) combine = combine + 10*b
end function combine
pure subroutine set_optional(a, b, c)
integer, intent(in) :: a
integer, intent(out) :: c
integer, intent(in), optional :: b
c = a
if (present(b)) c = c + b
end subroutine set_optional
end module named_arguments_module

program main
use named_arguments_module
implicit none
real :: y, y2, y3
integer :: i
y = 10.0
call sub(y, x2=y2, x3=y3)
print *, y, y2, y3
call sub(X3=y3, X=4.0, X2=y2)
print *, y2, y3
print *, encode(c=3, a=1, b=2), encode(4, c=6, b=5)
print *, combine(c=3, a=1), combine(c=3, b=2, a=1)
call set_optional(a=4, c=i)
print *, i
call set_optional(c=i, b=5, a=4)
print *, i
end program main
