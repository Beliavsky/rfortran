program u1
implicit none
integer :: i, n, k
real :: s, a(5)
n = 5
do i = 1, n
  if (i > 3) s = s + i
end do
print *, s
a(1:4) = 1.0
print *, sum(a)
if (n > 10) k = 1
print *, k
end program
