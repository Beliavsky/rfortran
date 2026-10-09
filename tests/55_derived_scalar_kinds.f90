module samples
implicit none
type :: sample
integer :: serial = 7
real :: value = 1.5
double precision :: precise = 2.25d0
logical :: active = .true.
contains
procedure :: twice
end type sample
contains
pure real function twice(self)
class(sample), intent(in) :: self
twice = 2.0*self%value
end function twice
end module samples
program scalar_kinds
use samples
implicit none
type(sample) :: a, b
b = a
b%active = .false.
print '(i0,2f7.2,2l2)', b%serial, b%value, b%precise, a%active, b%active
print '(2f7.2)', a%twice(), b%precise
end program scalar_kinds
