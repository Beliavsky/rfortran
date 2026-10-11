program unbounded_do
implicit none
character(20) :: ch
integer :: values(3), i, temp, digit, outer, inner, count
values = [0, 123, -907]
do i = 1, 3
  ch = ''
  temp = abs(values(i))
  do ! Integer-to-string loop from util.f90's str_int.
    digit = mod(temp, 10) + 1
    ch = "0123456789"(digit:digit) // ch
    temp = temp / 10
    if (temp == 0) exit
  end do
  if (values(i) < 0) ch = '-' // trim(ch)
  print '(a)', trim(ch)
end do
outer = 0
count = 0
do; outer = outer + 1
  inner = 0
  do
    inner = inner + 1
    if (inner == 2) cycle
    if (inner == 4) exit
    count = count + 1
  end do
  if (outer < 3) cycle
  exit
end do
print *, outer, inner, count
end program unbounded_do
