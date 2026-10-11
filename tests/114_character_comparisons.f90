program character_comparisons
implicit none
character(4) :: a, b
character(0) :: empty
logical :: same
integer :: i
a = 'ab'
b = 'ac'
empty = ''
print *, a == b, a /= b, a < b, a <= b, a > b, a >= b
print *, a == 'ab', 'ab' == a, a == 'ab   ', a /= 'ab x'
print *, a < 'ab!', 'ab!' > a, 'z' > a, a < 'z'
print *, empty == '', empty == '   ', empty < 'a', 'a' > empty
print *, trim(a) == 'ab', trim(a)//'z' > b, a//'z' < 'ab z'
same = a == 'ab'
if (same .and. a /= b) print *, 123
i = 0
do while (a < b)
  i = i + 1
  a = b
end do
print *, i, a == b, a <= b, a >= b
a = 'b'
b = 'azzz'
print *, a > b, a < b
end program
