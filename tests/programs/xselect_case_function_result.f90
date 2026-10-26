module calendar
implicit none
contains
pure elemental integer function days_in_month(year, month) ! return number of days in a year-month pair
integer, intent(in) :: year, month
select case (month)
case (1,3,5,7,8,10,12)
   days_in_month = 31
case (4,6,9,11)
   days_in_month = 30
case (2)
   if ((mod(year,4) == 0 .and. mod(year,100) /= 0) .or. mod(year,400) == 0) then
      days_in_month = 29
   else
      days_in_month = 28
   end if
case default
   days_in_month = 0
end select
end function days_in_month
end module
program main
use calendar
implicit none
print *,days_in_month(2024,2),days_in_month(2023,2)
print *,days_in_month(2026,4),days_in_month(2026,1),days_in_month(2026,13)
end
