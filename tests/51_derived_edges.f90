module year_month_mod
   implicit none
   private

   public :: year_month, make_year_month

   type :: year_month
      integer :: year = 2000
      integer :: month = 1
   contains
      procedure :: valid
      procedure :: add_months
      procedure :: print => print_year_month
   end type year_month

contains

   ! Construct a year-month value.
   pure function make_year_month(year, month) result(ym)
      integer, intent(in) :: year
      integer, intent(in) :: month
      type(year_month) :: ym

      ym = year_month(year, month)
   end function make_year_month

   ! Check whether the month is valid.
   pure logical function valid(self)
      class(year_month), intent(in) :: self

      valid = self%month >= 1 .and. self%month <= 12
   end function valid

   ! Advance the date by n months (n may be negative).
   pure subroutine add_months(self, n)
      class(year_month), intent(inout) :: self
      integer, intent(in) :: n
      integer :: total

      total = 12*self%year + (self%month - 1) + n
      self%year = total / 12
      self%month = modulo(total, 12) + 1
      if (total < 0 .and. modulo(total, 12) /= 0) then
         self%year = self%year - 1
      end if
   end subroutine add_months

   ! Print the year and month.
   subroutine print_year_month(self)
      class(year_month), intent(in) :: self

      print '(i0,"-",i2.2)', self%year, self%month
   end subroutine print_year_month

end module year_month_mod


program derived_edges
use year_month_mod
implicit none
type(year_month) :: a, b
integer(8) :: big, divisor
call a%print()
b = a
b%year = 1999
call a%print()
call b%print()
a = year_month(0, 1)
call a%add_months(-1)
call a%print()
call a%add_months(-12)
call a%print()
call a%add_months(25)
call a%print()
a = year_month()
call a%print()
a = year_month(a%year + 1, a%month + 1)
call a%print()
a = make_year_month(a%year + 1, a%month + 1)
call a%print()
big = 9007199
big = big*1000000000 + 254740993
divisor = 12
print '(i0)', modulo(big, divisor)
print *, modulo(-13,12)
print '(4i5)', modulo(-13,12), modulo(13,-12), modulo(-24,12), modulo(24,-12)
print '(4f8.2)', modulo(-13.5d0,12.0d0), modulo(13.5d0,-12.0d0), modulo(-24.0d0,12.0d0), modulo(24.0d0,-12.0d0)
print '("Case,Upper:",i4.2,"%",i2.2)', -3, 4
print '(i3.0,i0.0,"END")', 0, 0
end program derived_edges
