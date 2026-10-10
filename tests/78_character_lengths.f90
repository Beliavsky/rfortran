module character_sizes
implicit none
integer, parameter :: n=3
character(len=*), parameter :: label='ab  '
character(len=2*n+1), parameter :: padded='abc'
contains
subroutine display()
character(len=n+1) :: local
local='123456'
print *, '['//local//']'
end subroutine display
pure integer function count_len() result(count)
character(len=3*n), parameter :: local='test'
count=len(local)
end function count_len
end module character_sizes

program character_lengths
use character_sizes, only: n,label,padded,display,count_len
implicit none
character(len=(n-1)*len(label)+len_trim(' z  ')) :: text
character(max(0,n-5)) :: empty
character(len=abs(2-n)+min(2,3)) :: short
character(len=4/2) :: chopped
character(len=-3) :: negative
print '(i0)', len(text)
text='abcdefghijklmnop'
short='q'
empty='ignored'
negative='ignored'
chopped='abcd'
print *, '['//text//']'
print *, '['//short//']'
print *, '['//empty//']'
print *, '['//negative//']'
print *, '['//chopped//']'
print *, '['//padded//']'
print '(i0)', len('abc')
call display()
print '(i0)', count_len()
end program character_lengths
