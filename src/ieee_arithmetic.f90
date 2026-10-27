! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
module ieee_arithmetic
implicit none
private
public :: ieee_class_type, ieee_value, ieee_is_nan, ieee_is_finite, ieee_is_normal
public :: ieee_unordered
 type ieee_class_type
 integer :: code
 end type
type(ieee_class_type), parameter, public :: ieee_signaling_nan = ieee_class_type(1)
type(ieee_class_type), parameter, public :: ieee_quiet_nan = ieee_class_type(2)
type(ieee_class_type), parameter, public :: ieee_negative_inf = ieee_class_type(3)
type(ieee_class_type), parameter, public :: ieee_negative_normal = ieee_class_type(4)
type(ieee_class_type), parameter, public :: ieee_negative_denormal = ieee_class_type(5)
type(ieee_class_type), parameter, public :: ieee_negative_zero = ieee_class_type(6)
type(ieee_class_type), parameter, public :: ieee_positive_zero = ieee_class_type(7)
type(ieee_class_type), parameter, public :: ieee_positive_denormal = ieee_class_type(8)
type(ieee_class_type), parameter, public :: ieee_positive_normal = ieee_class_type(9)
type(ieee_class_type), parameter, public :: ieee_positive_inf = ieee_class_type(10)
type(ieee_class_type), parameter, public :: ieee_negative_subnormal = ieee_class_type(5)
type(ieee_class_type), parameter, public :: ieee_positive_subnormal = ieee_class_type(8)
public :: ieee_signaling_nan
public :: ieee_quiet_nan
public :: ieee_negative_inf
public :: ieee_negative_normal
public :: ieee_negative_denormal
public :: ieee_negative_zero
public :: ieee_positive_zero
public :: ieee_positive_denormal
public :: ieee_positive_normal
public :: ieee_positive_inf
public :: ieee_negative_subnormal
public :: ieee_positive_subnormal
interface ieee_value
module procedure ieee_value4, ieee_value8
end interface
interface ieee_is_nan
module procedure ieee_is_nan4, ieee_is_nan8
end interface
interface ieee_is_finite
module procedure ieee_is_finite4, ieee_is_finite8
end interface
interface ieee_is_normal
module procedure ieee_is_normal4, ieee_is_normal8
end interface
interface ieee_unordered
module procedure ieee_unordered4, ieee_unordered8
end interface
contains
elemental function ieee_value4(x,class) result(res)
real(kind=4), intent(in) :: x
type(ieee_class_type), intent(in) :: class
real(kind=4) :: res
! X selects the result kind; its stored value is not an input.
res = ieee_bits4(0.0_4,class%code)
end function
elemental function ieee_is_nan4(x) result(res)
real(kind=4), intent(in) :: x
logical :: res
res = x /= x
end function
elemental function ieee_is_finite4(x) result(res)
real(kind=4), intent(in) :: x
logical :: res
res = abs(x) <= huge(x)
end function
elemental function ieee_is_normal4(x) result(res)
real(kind=4), intent(in) :: x
logical :: res
res = x == 0 .or. (abs(x) >= tiny(x) .and. abs(x) <= huge(x))
end function
elemental function ieee_unordered4(x,y) result(res)
real(kind=4), intent(in) :: x
logical :: res
real(kind=4), intent(in) :: y
res = x /= x .or. y /= y
end function
elemental function ieee_value8(x,class) result(res)
real(kind=8), intent(in) :: x
type(ieee_class_type), intent(in) :: class
real(kind=8) :: res
! X selects the result kind; its stored value is not an input.
res = ieee_bits8(0.0_8,class%code)
end function
elemental function ieee_is_nan8(x) result(res)
real(kind=8), intent(in) :: x
logical :: res
res = x /= x
end function
elemental function ieee_is_finite8(x) result(res)
real(kind=8), intent(in) :: x
logical :: res
res = abs(x) <= huge(x)
end function
elemental function ieee_is_normal8(x) result(res)
real(kind=8), intent(in) :: x
logical :: res
res = x == 0 .or. (abs(x) >= tiny(x) .and. abs(x) <= huge(x))
end function
elemental function ieee_unordered8(x,y) result(res)
real(kind=8), intent(in) :: x
logical :: res
real(kind=8), intent(in) :: y
res = x /= x .or. y /= y
end function
end module
