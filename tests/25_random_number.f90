program main
implicit none
real :: x
real :: a(4)
double precision :: d
double precision :: b(3)
integer :: i
x = -1.0
call random_number(x)
print *, x, x + 0.0
do i = 1, 5
   call random_number(x)
   print *, x >= 0.0 .and. x < 1.0
end do
call RANDOM_NUMBER(d)
print *, d >= 0.0 .and. d < 1.0
call random_number(a)
print *, a
call random_number(b)
print *, b
i = 2
call random_number(a(i))
print *, a
call random_number(b(1))
print *, b
end program main
