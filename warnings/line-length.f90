! Warning:    line-length
! Enabled by: -Wstyle
! A free-form line longer than 132 characters, the limit of the standard
! before Fortran 2023, or than N with -Wline-length=N. Use & to continue the
! statement. The warning is given once per file.
program line_length
implicit none
real :: first_value, second_value, third_value, total_value
first_value = 1.0
second_value = 2.0
third_value = 3.0
total_value = first_value + second_value + third_value + first_value*second_value + second_value*third_value + first_value*third_value  ! expect: line-length
print *, total_value
total_value = first_value + second_value + third_value &
   + first_value*second_value + second_value*third_value
print *, total_value
end program line_length
