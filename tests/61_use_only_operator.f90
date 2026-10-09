module selected_box
implicit none
private
public :: box, operator(+)
type :: box
integer :: value
end type box
interface operator(+)
module procedure add_box
end interface
contains
function add_box(a, b) result(c)
type(box), intent(in) :: a, b
type(box) :: c
c%value = a%value + b%value
end function add_box
end module selected_box

program selected_operator
use selected_box, only: box, operator(+)
implicit none
type(box) :: a, b, c
a = box(7)
b = box(8)
c = a + b
print *, c%value
end program selected_operator
