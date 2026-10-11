program main
implicit none
integer :: iter, n1, n2, ny
real :: x
real, allocatable :: y(:,:)
do iter=1,3
   call random_number(x)
   if (x > 0.5) then
      n1 = 3
      n2 = 4
   else
      n1 = 10
      n2 = 20
   end if
   if (allocated(y)) deallocate(y)
   allocate (y(n1,n2))
   call random_number(y)
   ny = size(y)
   print*,x,ny,sum(y)/y
end do
end program main
