program main
implicit none
type table
character(8), allocatable :: columns(:)
end type
type(table) :: t
integer :: i
allocate(t%columns(3))
do i=1,3
write(t%columns(i), "('C',i0)") i
end do
print '(a)', '['//t%columns(1)//']', '['//t%columns(2)//']', '['//t%columns(3)//']'
end program
