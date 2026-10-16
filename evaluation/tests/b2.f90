program b2
implicit none
real(8) :: s
integer :: it
s = 0
do it = 1, 200000000
  s = s + sqrt(real(it,8))
end do
print *, s
end program
