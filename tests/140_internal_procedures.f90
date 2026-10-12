module internal_helpers
contains
subroutine reverse_values(values)
integer, intent(inout) :: values(:)
call reverse_range(1,size(values))
contains
recursive subroutine reverse_range(left,right)
integer, intent(in) :: left,right
integer :: temp
if (left>=right) return
temp=values(left)
values(left)=values(right)
values(right)=temp
call reverse_range(left+1,right-1)
end subroutine reverse_range
end subroutine reverse_values
end module internal_helpers

program main
use internal_helpers
implicit none
integer :: values(5)
values=[1,2,3,4,5]
call reverse_values(values)
print *,values
call reverse_values(values)
print *,values
end program main
