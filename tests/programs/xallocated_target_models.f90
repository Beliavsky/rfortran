module allocation_models
implicit none
type payload
   integer :: n = 1
   real, allocatable :: data(:)
end type
type entry
   class(payload), pointer :: object => null()
end type
type(payload), allocatable, target :: target
end module

program p
use allocation_models
implicit none
type(payload) :: value
type(entry) :: view
value%n = 7
value%data = [2.0, 4.0]

allocate(target, source=value)
view%object => target
target%data(1) = 9.0
print *, view%object%n, sum(view%object%data), sum(value%data)
nullify(view%object)
deallocate(target)

allocate(target, mold=value)
print *, target%n, allocated(target%data)
deallocate(target, value%data)
end
