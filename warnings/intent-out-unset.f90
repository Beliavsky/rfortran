! Warning:    intent-out-unset
! Enabled by: -Wextra
! An INTENT(OUT) argument that some path through the procedure does not
! assign, so the caller receives an undefined value.
program intent_out_unset
implicit none
real :: r
call safe_sqrt(4.0, r)
print *, r
call safe_sqrt_always(-4.0, r)
print *, r
contains
subroutine safe_sqrt(x, root)
real, intent(in) :: x
real, intent(out) :: root      ! expect: intent-out-unset
if (x >= 0) root = sqrt(x)
end subroutine safe_sqrt
subroutine safe_sqrt_always(x, root)
real, intent(in) :: x
real, intent(out) :: root
root = 0.0
if (x >= 0) root = sqrt(x)
end subroutine safe_sqrt_always
end program intent_out_unset
