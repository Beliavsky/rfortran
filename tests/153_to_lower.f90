module m
implicit none
contains
pure function to_lower(str) result(lower)
   implicit none
   character(*), intent(in) :: str
   character(len(str)) :: lower
   integer :: i, c

   lower = str
   do i = 1, len(str)
      c = iachar(str(i:i))
      if (c >= iachar('A') .and. c <= iachar('Z')) then
         lower(i:i) = achar(c + 32)
      end if
   end do
end function to_lower
end module m

use m
print*,to_lower("JimBob")
end
