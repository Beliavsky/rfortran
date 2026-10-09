! echo: 1
program main
implicit none
integer :: choice, x, a(2), b(2), i
read(*,*) choice
if (choice > 0) then
   x = 42
else
   x = 17
end if
print *, x
if (choice > 0) then
   x = 9
end if
print *, x
do i = 1,2
   a(i) = i*10
end do
print *, a
b(1) = 1
if (choice > 0) then
   b(2) = 0
end if
i = 1
do while (b(i) > 0)
   i = i + 1
   cycle
end do
print *, 7
end program main
