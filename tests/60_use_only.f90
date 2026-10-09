module selected_base
implicit none
private
public :: dp, offset, twice
integer, parameter :: dp = selected_real_kind(15,307), offset = 7
integer, parameter :: hidden = 3
contains
integer function twice(i)
integer :: i
twice = helper(i) + helper(i)
end function twice
integer function helper(i)
integer :: i
helper = i + hidden
end function helper
end module selected_base

module selected_middle
use selected_base, only: dp, offset, twice
implicit none
integer, parameter :: total = offset + 1
contains
integer function answer(i)
integer :: i
answer = twice(i) + total
end function answer
end module selected_middle

program selected_main
use selected_middle, only: dp, offset, answer
implicit none
real(kind=dp) :: value
integer :: hidden
hidden = 100
value = 1.5_dp
print *, offset, answer(4), hidden, value
end program selected_main
