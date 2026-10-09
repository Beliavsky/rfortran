module year_month_mod
   implicit none
   private

   public :: year_month, make_year_month
   public :: operator(+), operator(-)
   public :: operator(<), operator(<=), operator(==)
   public :: operator(>=), operator(>)

   type :: year_month
      integer :: year = 2000
      integer :: month = 1
   contains
      procedure :: valid
      procedure :: add_months
      procedure :: print => print_year_month
   end type year_month

   interface operator(+)
      module procedure add_months_right
      module procedure add_months_left
   end interface

   interface operator(-)
      module procedure subtract_months
      module procedure difference_months
   end interface

   interface operator(<)
      module procedure less_than
   end interface

   interface operator(<=)
      module procedure less_equal
   end interface

   interface operator(==)
      module procedure equal
   end interface

   interface operator(>=)
      module procedure greater_equal
   end interface

   interface operator(>)
      module procedure greater_than
   end interface

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

   ! Return a serial month number.
   pure integer function serial_month(ym) result(n)
      type(year_month), intent(in) :: ym

      n = 12*ym%year + ym%month - 1
   end function serial_month

   ! Advance the date by n months.
   pure subroutine add_months(self, n)
      class(year_month), intent(inout) :: self
      integer, intent(in) :: n
      integer :: total

      total = 12*self%year + self%month - 1 + n

      self%year = (total - modulo(total, 12))/12
      self%month = modulo(total, 12) + 1
   end subroutine add_months

   ! Add months: date + integer.
   pure function add_months_right(ym, n) result(res)
      type(year_month), intent(in) :: ym
      integer, intent(in) :: n
      type(year_month) :: res

      res = ym
      call res%add_months(n)
   end function add_months_right

   ! Add months: integer + date.
   pure function add_months_left(n, ym) result(res)
      integer, intent(in) :: n
      type(year_month), intent(in) :: ym
      type(year_month) :: res

      res = ym
      call res%add_months(n)
   end function add_months_left

   ! Subtract months: date - integer.
   pure function subtract_months(ym, n) result(res)
      type(year_month), intent(in) :: ym
      integer, intent(in) :: n
      type(year_month) :: res

      res = ym
      call res%add_months(-n)
   end function subtract_months

   ! Difference between dates in months: date - date.
   pure integer function difference_months(a, b) result(n)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      n = serial_month(a) - serial_month(b)
   end function difference_months

   ! Chronological comparisons.
   pure logical function less_than(a, b)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      less_than = serial_month(a) < serial_month(b)
   end function less_than

   pure logical function less_equal(a, b)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      less_equal = serial_month(a) <= serial_month(b)
   end function less_equal

   pure logical function equal(a, b)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      equal = a%year == b%year .and. a%month == b%month
   end function equal

   pure logical function greater_equal(a, b)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      greater_equal = serial_month(a) >= serial_month(b)
   end function greater_equal

   pure logical function greater_than(a, b)
      type(year_month), intent(in) :: a
      type(year_month), intent(in) :: b

      greater_than = serial_month(a) > serial_month(b)
   end function greater_than

   ! Print a date in YYYY-MM format.
   subroutine print_year_month(self)
      class(year_month), intent(in) :: self

      print '(i0,"-",i2.2)', self%year, self%month
   end subroutine print_year_month

end module year_month_mod


program test_year_month
   use year_month_mod
   implicit none

   type(year_month) :: a, b, c

   a = year_month(2026, 10)
   b = year_month(2027, 3)

   print *, "Date a:"
   call a%print()

   print *, "Date b:"
   call b%print()

   ! Addition.
   c = a + 5
   print *, "a + 5:"
   call c%print()

   if (.not. (c == b)) error stop "addition failed"

   c = 5 + a
   print *, "5 + a:"
   call c%print()

   if (.not. (c == b)) error stop "left addition failed"

   ! Subtraction of months.
   c = b - 5
   print *, "b - 5:"
   call c%print()

   if (.not. (c == a)) error stop "subtraction failed"

   ! Difference between dates.
   print *, "b - a =", b - a
   print *, "a - b =", a - b

   if (b - a /= 5) error stop "date difference failed"
   if (a - b /= -5) error stop "negative difference failed"

   ! Comparisons.
   print *, "a <  b:", a < b
   print *, "a <= b:", a <= b
   print *, "a == b:", a == b
   print *, "a >= b:", a >= b
   print *, "a >  b:", a > b

   if (.not. (a < b)) error stop "less than failed"
   if (.not. (a <= b)) error stop "less equal failed"
   if (a == b) error stop "equality failed"
   if (a >= b) error stop "greater equal failed"
   if (a > b) error stop "greater than failed"

   ! Equality and boundary comparisons.
   c = a

   if (.not. (a == c)) error stop "equal dates failed"
   if (.not. (a <= c)) error stop "equal less failed"
   if (.not. (a >= c)) error stop "equal greater failed"
   if (a < c) error stop "strict less failed"
   if (a > c) error stop "strict greater failed"

   ! Crossing year boundaries.
   c = year_month(2026, 1) - 2
   print *, "2026-01 minus 2 months:"
   call c%print()

   if (.not. (c == year_month(2025, 11))) then
      error stop "year boundary failed"
   end if

   ! Negative month differences across years.
   if (year_month(2025, 11) - year_month(2026, 1) /= -2) then
      error stop "cross-year difference failed"
   end if

   ! Validation.
   c = year_month(2026, 13)
   if (c%valid()) error stop "invalid month accepted"

   print *, "All tests passed."

end program test_year_month