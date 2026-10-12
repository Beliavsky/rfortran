program formatted_implied_do
implicit none
integer :: i
integer, parameter :: nlen = 5
character(nlen) :: names(3) = [character(len=nlen) :: 'alice', 'bob', 'cathy']
print '(*(1x,a))', (trim(names(i)), i=1,size(names))
print '(*(1x,a))', 'alice', 'bob', 'cathy'
end program formatted_implied_do
