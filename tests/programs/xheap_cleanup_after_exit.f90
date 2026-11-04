program main
implicit none
integer :: i
do i=1,3
   block
      real, allocatable :: x(:)
      allocate(x(i))
      x=2
      print *,sum(x)
      exit
   end block
end do
print *, 'passed'
end program
