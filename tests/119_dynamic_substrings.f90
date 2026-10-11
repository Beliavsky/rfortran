implicit none
character(len=:), allocatable :: s
integer :: i1=3, i2=7
s = "0123456789"(i1:i2)
print "(a)", s
print *, len(s), s == '23456'
s = 'x'
print '(a,a,a)', '[', s, ']'
s = ''
print *, len(s), s == ''
end
