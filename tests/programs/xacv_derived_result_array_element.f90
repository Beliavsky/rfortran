! Regression for the former xacv.f90 blocker at line 1172: a scalar derived result assigned to an array element.
module month_year_result_mod
implicit none
type :: month_year
   integer :: month=0,year=0
end type
contains
function from_code(code) result(value)
integer,intent(in)::code
type(month_year)::value
value%year=code/100
value%month=modulo(code,100)
end function
end module
program test
use month_year_result_mod
implicit none
type(month_year)::values(2)
integer::i
do i=1,2
   values(i)=from_code(202400+i)
end do
print *,values%year,values%month
! Expected: 2024 2024 1 2
end program
