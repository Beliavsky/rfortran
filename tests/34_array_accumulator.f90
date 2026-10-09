integer :: a(2), total, i
a(1) = 2
a(2) = 3
total = 0
print *, total
do i = 1,2
   total = total + a(i)
end do
print *, total
end
