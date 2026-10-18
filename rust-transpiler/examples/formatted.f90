program formatted
   implicit none
   integer, parameter :: n = 5
   integer :: i
   do i=1,n
      print "(2i5,f10.6)", i, i**2, sqrt(real(i))
   end do
end program formatted
