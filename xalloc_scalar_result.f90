program allocatable_scalar_result
implicit none
integer, allocatable :: x

x = make_value(3)
print *, allocated(x), x
x = make_value(4)
print *, x
print *, make_value(2) + make_value(5)

contains

function make_value(n) result(value)
integer, intent(in) :: n
integer, allocatable :: value
allocate(value)
value = n*n
end function make_value

end program allocatable_scalar_result
