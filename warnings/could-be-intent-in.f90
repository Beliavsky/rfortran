! Warning:    could-be-intent-in
! Enabled by: -Wextra
! An INTENT(INOUT) argument that nothing in the procedure can modify.
! INTENT(IN) tells callers it is unchanged, and lets them pass constants
! and expressions.
program could_be_intent_in
implicit none
real :: x(3) = [1.0, 2.0, 3.0]
call show_total(x)
call scale(x)
print *, x
contains
subroutine show_total(v)
real, intent(inout) :: v(:)       ! expect: could-be-intent-in
print *, sum(v)
end subroutine show_total
subroutine scale(v)
real, intent(inout) :: v(:)
v = 2*v
end subroutine scale
end program could_be_intent_in
