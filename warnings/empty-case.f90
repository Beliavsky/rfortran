! Warning:    empty-case
! Enabled by: -Wall
! A CASE range whose lower bound exceeds its upper bound, as case (5:1),
! matches nothing. It was probably meant to be case (1:5).
program empty_case
implicit none
integer :: n
n = 3
select case (n)
case (1:2)
   print *, "small"
case (5:3)           ! expect: empty-case
   print *, "medium"
case default
   print *, "other"
end select
end program empty_case
