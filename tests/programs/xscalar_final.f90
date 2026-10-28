module final_demo
implicit none
type :: resource
   integer :: id = 0
   integer, allocatable :: data(:)
contains
   final :: release
end type
contains
subroutine release(x)
   type(resource), intent(inout) :: x
   print *, 'final', x%id, allocated(x%data)
end subroutine

subroutine reset(x)
   type(resource), intent(out) :: x
   print *, 'reset', x%id, allocated(x%data)
   x%id = 2
   allocate(x%data(1))
   x%data = 9
end subroutine

subroutine example()
   type(resource) :: x
   x%id = 1
   allocate(x%data(2))
   call reset(x)
   x = resource(3, [4,5])
   print *, x%id, sum(x%data)
end subroutine
end module

program demo
use final_demo
implicit none
call example()
print *, 'done'
end program
