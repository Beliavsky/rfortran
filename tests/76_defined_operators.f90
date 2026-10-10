module defined_ops
implicit none
private
public :: operator(.plus.), operator(.twice.), operator(.in.), operator(.notall.), nested
interface operator(.plus.)
module procedure add_int, add_real
end interface operator(.plus.)
interface operator(.twice.)
module procedure twice_int
end interface
interface operator(.in.)
module procedure equal_int
end interface
interface operator(.notall.)
module procedure not_both
end interface
contains
pure integer function nested(a) result(c)
integer, intent(in) :: a
c=a .plus. (.twice. a)
end function nested
pure integer function add_int(a,b) result(c)
integer, intent(in) :: a,b
c=a+b
end function add_int
pure real(8) function add_real(a,b) result(c)
real(8), intent(in) :: a,b
c=a+b
end function add_real
pure integer function twice_int(a) result(c)
integer, intent(in) :: a
c=2*a
end function twice_int
pure logical function equal_int(a,b) result(c)
integer, intent(in) :: a,b
c=a==b
end function equal_int
pure logical function not_both(a,b) result(c)
logical, intent(in) :: a,b
c=.not.(a.and.b)
end function not_both
end module defined_ops

module public_ops
use defined_ops, only: operator(.plus.), operator(.twice.), operator(.in.), operator(.notall.), nested
implicit none
end module public_ops

program test_defined
use public_ops, only: operator(.plus.), operator(.twice.), operator(.in.), operator(.notall.), nested
implicit none
integer :: x
x=2 .PLUS. 3*4
print '(i0)', x
print '(i0)', .twice. 3**2
print '(i0)', .twice. (3**2)
print '(i0)', 1 .plus. 2 .plus. 3
print '(f5.1)', 1.0d0 .plus. 2.5d0
print '(l1)', .true. .or. .false. .notall. .true.
if (x .in. 14) print '(a)', 'matched'
print '(i0)', 2*(1 .plus. 4)
print '(i0)', nested(4)
end program test_defined
