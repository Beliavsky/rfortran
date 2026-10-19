program test_multiple_subscripts
implicit none
integer :: a(3,4), i, j
integer :: position(2) = [2,3]
integer :: lower(2) = [1,2], upper(2) = [3,4]
do j=1,4
   do i=1,3
      a(i,j)=10*j+i
   end do
end do
print *, "element:", a(@position)
print *, "section:", a(@lower:upper:2)
a(@position)=99
print *, "updated:", a(2,3)
end program
