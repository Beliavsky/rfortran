module m2
   use m1
   implicit none
   integer, parameter :: offset = base + 1
contains
   pure integer function answer()
      answer = twice(offset)
   end function answer
end module m2
