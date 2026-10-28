module owned_output_demo
implicit none
type :: row
   integer :: id = 7
   integer, allocatable :: values(:)
end type
contains
subroutine reset(rows)
   type(row), intent(out) :: rows(:)
   integer :: i
   do i = 1, size(rows)
      print *, allocated(rows(i)%values), rows(i)%id
      allocate(rows(i)%values(2))
      rows(i)%values = [i, 10*i]
      rows(i)%id = 99
   end do
end subroutine
end module

program demo
use owned_output_demo
implicit none
type(row) :: rows(-1:0)
allocate(rows(-1)%values(3), rows(0)%values(0))
call reset(rows)
call reset(rows)
print *, lbound(rows,1), ubound(rows,1)
print *, rows(-1)%values, rows(0)%values
end program
