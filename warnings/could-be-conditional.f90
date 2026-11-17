! Warning:    could-be-conditional
! Enabled by: -Weverything
! An IF block whose branches each assign one value to the same variable,
! which a Fortran 2023 conditional expression writes in one statement:
! y = (x > 0 ? sqrt(x) : -1.0). Only for compilers that support Fortran
! 2023.
program could_be_conditional
implicit none
real :: x, y
x = 4.0
if (x > 0) then              ! expect: could-be-conditional
   y = sqrt(x)
else
   y = -1.0
end if
print *, y
if (x > 0) then
   y = sqrt(x)
   print *, "positive"
else
   y = -1.0
end if
print *, y
end program could_be_conditional
