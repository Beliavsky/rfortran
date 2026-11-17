! Warning:    shadow
! Enabled by: -Wextra
! A local declaration that hides a variable or named constant of the host
! or of a used module. References in the procedure then mean the local,
! which may not be what the author of a later change expects.
program shadow
implicit none
integer :: count_total
count_total = 5
call report()
contains
subroutine report()
integer :: count_total            ! expect: shadow
count_total = 1
print *, count_total
end subroutine report
end program shadow
