! Regression: a polymorphic pointer component in a record-array element.
program test_pointer_array_component
implicit none
type base
integer::value
end type
type entry
class(base),pointer::object=>null()
end type
type(entry),dimension(2)::list
type(base),target::one
one%value=7
list(1)%object=>one
print *,associated(list(1)%object),list(1)%object%value
end
