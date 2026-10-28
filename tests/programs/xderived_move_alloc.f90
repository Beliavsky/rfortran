module move_records
implicit none
type record
   integer :: value = 1
   real, allocatable :: data(:)
   character(len=5), allocatable :: label(:)
end type
end module

program test_move
use move_records
implicit none
type(record), allocatable, target :: from, to
allocate(from)
from%value = 7
from%data = [2.0, 3.0]
from%label = ['moved']
allocate(to)
to%data = [99.0]
call move_alloc(from, to)
print *, allocated(from), allocated(to), to%value
print *, to%data
print *, to%label
deallocate(to)
end program
