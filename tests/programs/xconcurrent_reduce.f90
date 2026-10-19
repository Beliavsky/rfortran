implicit none
integer :: i, total, largest, counts(2)
total = 10
largest = -huge(largest)
counts = 0
do concurrent (i=1:5) reduce(+:total,counts) reduce(max:largest)
   total = total + i
   counts = counts + [1,i]
   largest = max(largest,i)
end do
print *, total, largest, counts
end
