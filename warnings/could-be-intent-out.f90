! Warning:    could-be-intent-out
! Enabled by: -Wextra
! An INTENT(INOUT) argument whose value on entry is never used: every path
! assigns it before reading it. INTENT(OUT) says so to callers and lets the
! compiler check it.
program could_be_intent_out
implicit none
integer :: n
n = 0
call set_count(n)
call increment(n)
print *, n
contains
subroutine set_count(c)
integer, intent(inout) :: c       ! expect: could-be-intent-out
c = 10
end subroutine set_count
subroutine increment(c)
integer, intent(inout) :: c
c = c + 1
end subroutine increment
end program could_be_intent_out
