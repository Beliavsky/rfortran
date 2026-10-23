! Regression for xacv.f90 line 15947: formatted output of derived records.
program test
implicit none
type :: date_mdy
integer :: month,day,year
end type
type(date_mdy) :: dates(2,2)
dates(:,1)=date_mdy(1,2,2024)
dates(:,2)=date_mdy(3,4,2025)
write(*,"(2(i2.2,'/'),i4.4,1x,2(i2.2,'/'),i4.4)") dates(1,:)
! Expected: 01/02/2024 03/04/2025
end program
