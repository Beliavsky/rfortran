! Reproduce util.f90:upper_case_str's substring of a CHARACTER named constant.
! Expected output: JIMBOB 09!.
module parameter_substring
implicit none
character(len=*), parameter :: lower_case = 'abcdefghijklmnopqrstuvwxyz'
character(len=*), parameter :: upper_case = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
contains
elemental function upper_case_str(input_string) result(output_string)
character(len=*), intent(in) :: input_string
character(len=len(input_string)) :: output_string
integer :: i, n
output_string = input_string
do i = 1, len(output_string)
   n = index(lower_case, output_string(i:i))
   if (n /= 0) output_string(i:i) = upper_case(n:n)
end do
end function upper_case_str
end module parameter_substring

program test_parameter_substring
use parameter_substring
implicit none
print '(a)', upper_case_str('JimBob 09!')
end program test_parameter_substring
