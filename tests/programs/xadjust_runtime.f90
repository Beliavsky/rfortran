program main
implicit none
character(len=8) :: text = 'x  ab  y'
character(len=:), allocatable :: left, right
integer :: first, last
first = 2
last = 7
left = adjustl(text(first:last))
right = adjustr(string=text(first:last))
print '(a)', '|' // left // '|', '|' // right // '|'
print *, len(left), len(right)
print '(a)', '|' // trim(adjustl(text(first:last))) // '|'
end program main
