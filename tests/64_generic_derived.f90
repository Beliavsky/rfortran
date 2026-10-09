module generic_records
implicit none
private
public :: pick, record
type :: record
integer :: value = 7
end type record
interface pick
module procedure pick_record, pick_character, pick_logical
end interface pick
contains
integer function pick_record(x)
type(record), intent(in) :: x
pick_record = x%value
end function pick_record
integer function pick_character(x)
character(len=3) :: x
pick_character = 0
if (x == 'abc') pick_character = 3
end function pick_character
integer function pick_logical(x)
logical :: x
pick_logical = 0
if (x) pick_logical = 1
end function pick_logical
end module generic_records

module generic_extended
use generic_records, only: pick, record
implicit none
interface pick
module procedure pick_real
end interface &
 & pick
contains
integer function pick_real(x)
real :: x
pick_real = int(x)
end function pick_real
end module generic_extended

program test_generic_records
use generic_extended, only: pick, record
implicit none
type(record) :: x
print *, pick(x), pick(record(8)), pick('abc'), pick(.true.), pick(9.0)
end program test_generic_records
