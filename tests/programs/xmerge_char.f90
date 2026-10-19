implicit none
character (len=5) :: s
integer :: i
real :: x
do i=1,5
   call random_number(x)
   s = merge("big  ", "small", x > 0.5)
   print "(a6, f6.3)", s, x
end do
end

