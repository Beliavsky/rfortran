module search_mod
implicit none
contains
pure integer function last_true(tf) result(pos)
logical, intent(in) :: tf(:)
integer :: i
pos=0
do i=size(tf),1,-1
   if (tf(i)) then
      pos=i
      return
   end if
end do
end function
end module
program main
use search_mod
implicit none
real, allocatable :: x(:)
integer :: n
read *,n
allocate(x(n))
x=0
if (n>=2) x(2)=1
print *,last_true(x>0)
end program
