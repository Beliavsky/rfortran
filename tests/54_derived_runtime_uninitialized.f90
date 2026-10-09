! echo: 0
implicit none
type :: pair
integer :: first = 1
integer :: second
end type pair
type(pair) :: p
integer :: choice
read(*,*) choice
if (choice == 1) p%second = 9
print *, p%first
print *, p%second
end
