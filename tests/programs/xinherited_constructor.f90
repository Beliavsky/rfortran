module inherited_constructor_types
implicit none
type :: base
   integer :: x=1, y=2
end type
type, extends(base) :: child
   integer :: z=3
end type
end module

program main
use inherited_constructor_types
implicit none
type(child) :: value
type(child), allocatable, target :: target
value=child(base(4,5),20)
allocate(target,source=child(base=base(4,5),z=20))
print *,value%x,value%y,value%z
print *,target%x,target%y,target%z
end program
