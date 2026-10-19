implicit none
integer, parameter :: n = 5
character (len=5) :: s(n)
integer :: i
real :: x(n)
call random_number(x)
s = merge("big  ", "small", x > 0.5)
do i=1,5
   print "(a6, f6.3)", s(i), x(i)
end do
end