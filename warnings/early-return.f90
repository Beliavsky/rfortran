! Warning:    early-return
! Enabled by: -Weverything
! A RETURN after which statements of the procedure can still run, as
! `if (n <= 0) return`. Some styles keep one exit per procedure, at its
! end, so that cleanup code always runs. A final RETURN is not reported.
program early_return
implicit none
call show(3)
call show_nested(3)
contains
subroutine show(n)
integer, intent(in) :: n
if (n <= 0) return               ! expect: early-return
print *, n
end subroutine show
subroutine show_nested(n)
integer, intent(in) :: n
if (n > 0) then
   print *, n
end if
end subroutine show_nested
end program early_return
