! Reduced from xacv.f90 line 1494: ALL of an overloaded array comparison.
! Regression for elemental array dispatch after intrinsic generic resolution.
module acv_comparison_repro
implicit none
type :: date_type
   integer :: year
end type
interface operator(>)
   module procedure later
end interface
contains
elemental logical function later(a,b)
   type(date_type), intent(in) :: a,b
   later=a%year>b%year
end function
end module
program test
use acv_comparison_repro
implicit none
type(date_type) :: dates(3)
dates(1)%year=2020
dates(2)%year=2022
dates(3)%year=2024
print *,all(dates(2:) > dates(:2))
! Expected: T
end program
