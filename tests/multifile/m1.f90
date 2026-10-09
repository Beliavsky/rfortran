module m1
   implicit none
   integer, parameter :: base = 20
contains
   pure integer function twice(i)
      integer, intent(in) :: i
      twice = 2*i
   end function twice
end module m1
