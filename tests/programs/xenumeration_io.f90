program example
implicit none
enumeration type :: colour
   enumerator :: red, orange, green
end enumeration type
type(colour) :: light, colours(3)
integer :: status

light = orange
colours = [red, orange, green]
print '(a,i0)', 'initial=', light
print '(a,3(1x,i0))', 'colours:', colours
read (*,'(i3)',iostat=status) light
if (status == 0) then
   print '(a,i0)', 'read=', light
else
   print *, 'invalid ordinal or input error'
end if
end program example
