! SPDX-License-Identifier: MIT
module parallel_coefficients
implicit none
integer :: offset = 2
real, allocatable :: coefficients(:)
contains
pure real function evaluate(i) result(r)
integer, intent(in) :: i
r = real(i + offset)
if (allocated(coefficients)) r = r + coefficients(0)
end function
end module

program read_only_captures
use parallel_coefficients
implicit none
integer :: i
real :: y(9000)
do concurrent (i=1:9000)
   y(i) = evaluate(i)
end do
if (y(1) /= 3.0 .or. y(9000) /= 9002.0) stop 1
offset = 7
allocate(coefficients(-1:1))
coefficients = 3.0
do concurrent (i=1:9000)
   y(i) = evaluate(i)
end do
if (y(1) /= 11.0 .or. y(9000) /= 9010.0) stop 2
print *, 'PASS'
end program
