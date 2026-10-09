! echo: 0
module conditional_output
contains
subroutine maybe_set(choice,x)
integer, intent(in) :: choice
integer, intent(out) :: x
if(choice>0) then
x=7
end if
end subroutine maybe_set
end module conditional_output
program main
use conditional_output
implicit none
integer :: choice,x
read(*,*) choice
x=99
call maybe_set(choice,x)
print *,x
end program main
