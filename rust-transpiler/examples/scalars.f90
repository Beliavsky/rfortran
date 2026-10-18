program scalars
   implicit none
   integer, parameter :: n = 5
   integer :: i, total
   real :: x
   double precision :: y
   logical :: positive

   total = 0
   do i = 1, n
      total = total + i**2
   end do
   x = sqrt(real(total))
   y = log(exp(2.0d0))
   positive = total > 0 .and. x > 0.0
   if (positive) then
      print *, 'Sum of squares:', total
      print *, 'Square root:', x
   else
      print *, 'Unexpected nonpositive result'
   end if
   print *, 'Double result:', y
   print *, 'Positive:', positive
end program scalars
