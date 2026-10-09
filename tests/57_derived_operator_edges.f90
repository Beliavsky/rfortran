module year_month_mod
   implicit none
   private

   public :: year_month, make_year_month
   public :: operator(+), operator(-), operator(/=)
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
      module procedure add_months_left, identity_date, add_real_months
   end interface

   interface operator(-)
      module procedure subtract_months
      module procedure difference_months, negate_date
   end interface

   interface operator(.ne.)
      module procedure not_equal
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


   pure function add_real_months(a,n) result(res)
      type(year_month), intent(in) :: a
      real, intent(in) :: n
      type(year_month) :: res
      res=a+int(n)
   end function add_real_months
   pure function identity_date(a) result(res)
      type(year_month), intent(in) :: a
      type(year_month) :: res
      res=a
   end function identity_date
   pure function negate_date(a) result(res)
      type(year_month), intent(in) :: a
      type(year_month) :: res
      res=year_month(-a%year,a%month)
   end function negate_date
   pure logical function not_equal(a,b) result(res)
      type(year_month), intent(in) :: a,b
      res=.not.(a==b)
   end function not_equal

end module year_month_mod


program operator_edges
   use year_month_mod
   implicit none
   type(year_month)::a,b
   a=year_month(2026,10)
   b=(a+2)+3
   if (.not.(b==year_month(2027,3))) error stop "nested sum"
   if (.not.((a+2)+3 == 5+a)) error stop "nested comparison"
   if ((a+2)-(a-3) /= 5) error stop "nested difference"
   a=a+5
   if (a/=b) error stop "assignment alias"
   if (.not.(a .eq. b)) error stop "dotted eq"
   if (a .ne. b) error stop "dotted ne"
   if (.not.(a .le. b)) error stop "dotted le"
   if (.not.(a .ge. b)) error stop "dotted ge"
   if (a .lt. b) error stop "dotted lt"
   if (a .gt. b) error stop "dotted gt"
   a=+b
   if (a/=b) error stop "unary plus"
   a=-b
   if (a%year/=-2027) error stop "unary minus"
   if (a%month/=3) error stop "unary minus component"
   a=year_month(2000,1)
   b=a + (+5)
   if (b/=year_month(2000,6)) error stop "scalar unary plus"
   b=make_year_month(2020,12)+int(1.0)
   if (b/=year_month(2021,1)) error stop "function operands"
   b=year_month(2000,1)+2.4
   if (b/=year_month(2000,3)) error stop "real overload"
   b=year_month(2000,1)+sqrt(4.0)
   if (b/=year_month(2000,3)) error stop "intrinsic operand"
   print *, "Operator edge tests passed."
end program operator_edges
