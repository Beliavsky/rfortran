! Warning:    pointer-init
! Enabled by: -Wall
! A POINTER component of a derived type without a default initialization.
! Each new object then has an undefined pointer, which ASSOCIATED cannot
! test. Initialize it with => null().
program pointer_init
implicit none
type :: node
   integer :: value = 0
   type(node), pointer :: next             ! expect: pointer-init
   type(node), pointer :: previous => null()
end type node
type(node) :: first
print *, first%value, associated(first%previous)
end program pointer_init
