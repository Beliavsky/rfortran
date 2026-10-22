program short_circuit_warnings
implicit none
real, allocatable :: x(:)
real, target :: value = 1.0
real, pointer :: p
allocate(x(2))
x = 1.0
p => value
call show_array(x)
call show_pointer(p)
contains
subroutine show_array(x)
real, allocatable, intent(in) :: x(:)
! The interface does not require x to be allocated on entry.
if (allocated(x) .and. size(x)>0) print *, size(x)
end subroutine show_array
subroutine show_pointer(p)
real, pointer :: p
! ASSOCIATED alone does not guard the other operand of .AND.
if (associated(p) .and. p>0.0) print *, p
end subroutine show_pointer
end program short_circuit_warnings
