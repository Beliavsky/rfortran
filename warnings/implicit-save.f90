! Warning:    implicit-save
! Enabled by: -Wall
! A local variable initialized in its declaration, as `integer :: calls = 0`,
! has the SAVE attribute: it is set once, not on every call, and keeps its
! value between calls. Declare SAVE if that is intended, or assign the value
! in an executable statement.
program implicit_save
implicit none
call count_calls()
call count_calls()
contains
subroutine count_calls()
integer :: calls = 0               ! expect: implicit-save
integer, save :: saved_calls = 0
integer :: local
local = 0
calls = calls + 1
saved_calls = saved_calls + 1
local = local + 1
print *, calls, saved_calls, local
end subroutine count_calls
end program implicit_save
