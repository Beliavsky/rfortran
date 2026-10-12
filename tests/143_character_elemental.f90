module m
implicit none
contains
elemental function char_plus_char(c1,c2) result(c12)
! trim two strings and add them
character (len=*),   intent(in) :: c1,c2
character (len=len(c1)+len(c2)) :: c12
c12 = trim(c1) // trim(c2)
end function char_plus_char
end module m

use m
implicit none
print "(*(1x,a10))",char_plus_char(["ab", "cd", "ef"], "@")
end
