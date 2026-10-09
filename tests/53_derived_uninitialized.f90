implicit none
type :: pair
integer :: first = 1
integer :: second
end type pair
type(pair) :: p
print *, p%second
end
