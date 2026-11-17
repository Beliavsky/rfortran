! Warning:    target-lifetime
! Enabled by: -Wall
! A pointer associated with a local variable that ceases to exist when the
! procedure returns. Afterwards the pointer is undefined, and using it reads
! storage that may have been reused. A SAVE target lives on.
program target_lifetime
implicit none
integer, pointer :: p
call point_at_saved(p)
print *, p
call point_at_local(p)
contains
subroutine point_at_local(q)
integer, pointer, intent(out) :: q
integer, target :: local
local = 3
q => local    ! expect: target-lifetime
end subroutine point_at_local
subroutine point_at_saved(q)
integer, pointer, intent(out) :: q
integer, target, save :: kept = 4
q => kept
end subroutine point_at_saved
end program target_lifetime
