! Warning:    aliasing
! Enabled by: -Wall
! The same variable, or overlapping parts of it, passed as two arguments
! when the procedure modifies one of them. The standard forbids this, and
! the result depends on how the compiler passes the arguments.
program aliasing
implicit none
real :: x(4), y(4)
x = [1.0, 2.0, 3.0, 4.0]
y = 1.0
call add_reversed(x, y)
call add_reversed(x(1:2), x(3:4))
call add_reversed(x, x)          ! expect: aliasing
print *, x
contains
subroutine add_reversed(a, b)
real, intent(inout) :: a(:)
real, intent(in) :: b(:)
integer :: i
do i = 1, size(a)
   a(i) = a(i) + b(size(b) + 1 - i)
end do
end subroutine add_reversed
end program aliasing
