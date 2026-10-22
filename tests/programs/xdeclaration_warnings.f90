program declaration_warnings
implicit none
call count_calls(1)
call count_calls(2)
contains
subroutine count_calls(step)
integer :: step
integer :: count = 0
count = count + step
print *, count
end subroutine count_calls
end program declaration_warnings
