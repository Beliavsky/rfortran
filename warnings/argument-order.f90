! Warning:    argument-order
! Enabled by: -Wstyle
! An argument list with a required INTENT(IN) argument after an INTENT(OUT)
! or INTENT(INOUT) one, or a required argument after an OPTIONAL one. Inputs
! first, then outputs, then optional arguments is the usual order.
program argument_order
implicit none
real :: s
call add(s, 1.0, 2.0)
call add2(1.0, 2.0, s)
print *, s
contains
subroutine add(total, a, b)          ! expect: argument-order
real, intent(out) :: total
real, intent(in) :: a, b
total = a + b
end subroutine add
subroutine add2(a, b, total)
real, intent(in) :: a, b
real, intent(out) :: total
total = a + b
end subroutine add2
end program argument_order
