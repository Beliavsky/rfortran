! Warning:    missing-intent
! Enabled by: -Wextra, -Wstyle
! A dummy argument without INTENT. INTENT documents how the procedure uses
! the argument and lets the compiler check calls and the procedure body.
! Choose it from what the procedure does: INTENT(OUT) makes the argument
! undefined on entry.
program missing_intent
implicit none
real :: total
call add(2.0, 3.0, total)
print *, total
contains
subroutine add(a, b, s)
real :: a                        ! expect: missing-intent
real, intent(in) :: b
real, intent(out) :: s
s = a + b
end subroutine add
end program missing_intent
