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


program test_year_month
   use year_month_mod
   implicit none

   type(year_month) :: ym

   ym = make_year_month(2026, 10)

   print *, "Initial date:"
   call ym%print()

   print *, "Valid:", ym%valid()

   call ym%add_months(5)
   print *, "After adding 5 months:"
   call ym%print()

   call ym%add_months(-15)
   print *, "After subtracting 15 months:"
   call ym%print()

   ym = make_year_month(2026, 13)
   print *, "Month 13 valid:", ym%valid()

end program test_year_month
