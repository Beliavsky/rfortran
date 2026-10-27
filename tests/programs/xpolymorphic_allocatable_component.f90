! Reduced blocker from the FortranCon abstract-framework example.
module polymorphic_components
implicit none
type, abstract :: base
end type
type, extends(base) :: child
   integer :: value=7
end type
type :: container
   class(base), allocatable :: item
end type
end module
program test_polymorphic_component
use polymorphic_components
implicit none
type(container) :: box
type(child) :: initial
allocate(box%item,source=initial)
print *,allocated(box%item)
end program
