program allocatable_scalar_dummy
implicit none
integer, allocatable :: x

call set_value(x)
print *, allocated(x), x
call set_value(x) ! INTENT(OUT) releases the previous allocation.
print *, allocated(x), x
call clear_value(x)
print *, allocated(x)

contains

subroutine set_value(a)
integer, allocatable, intent(out) :: a
print *, 'allocated on entry:', allocated(a)
allocate(a)
a = 42
end subroutine set_value

subroutine clear_value(a)
integer, allocatable, intent(inout) :: a
if (allocated(a)) deallocate(a)
end subroutine clear_value

end program allocatable_scalar_dummy
