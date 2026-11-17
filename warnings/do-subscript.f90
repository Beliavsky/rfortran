! Warning:    do-subscript
! Enabled by: -Wall
! A subscript that a DO loop's range takes outside the array's declared
! bounds, as a(i) for i = 1 to 11 with a(10).
program do_subscript
implicit none
real :: a(10)
integer :: i
do i = 1, 10
   a(i) = i
end do
do i = 1, 11
   a(i) = 2*i     ! expect: do-subscript
end do
print *, sum(a)
end program do_subscript
