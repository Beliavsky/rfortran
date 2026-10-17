program test_now_format
implicit none
character(19) :: stamp
stamp = now()
print *, '['//stamp//']'
contains
function now() result(str)
character(19) :: str
integer :: ival(8)
ival = [2026,10,16,-240,9,8,7,0]
write(str,"(i4.4,2('-',i2.2),1x,i2.2,2(':',i2.2))") ival([1,2,3,5,6,7])
end function
end program
