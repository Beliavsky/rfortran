implicit none
integer, parameter :: n = 8
character (len=5), allocatable :: s(:)
integer :: i
real, allocatable :: x(:)
allocate (x(n), s(n))
call random_number(x)
s = merge("big  ", "small", x > 0.5)
do i=1,n
   print "(a6, f6.3)", s(i), x(i)
end do
end