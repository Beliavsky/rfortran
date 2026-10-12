module m
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
elemental function sgn_real(xx,thresh,smooth) result(yy)
! return sign of xx (-1,0,1)
real(kind=dp), intent(in)           :: xx
real(kind=dp), intent(in), optional :: thresh
logical      , intent(in), optional :: smooth
real(kind=dp)                       :: yy
if (present(thresh)) then
   if (abs(xx) < thresh) then
      if (present_and_false(smooth)) then
         yy = 0.0_dp
      else
         yy = xx/thresh
      end if
   else if (xx >= thresh) then
      yy =  1.0_dp
   else
      yy = -1.0_dp
   end if
else
   if (xx == 0.0_dp) then
      yy = 0.0_dp
   else if (xx > 0.0_dp) then
      yy = 1.0_dp
   else
      yy = -1.0_dp
   end if
end if
end function sgn_real

pure function present_and_false(xx) result(tf)
logical, intent(in), optional :: xx
logical                       :: tf
if (present(xx)) then
   tf = .not. xx
else
   tf = .false.
end if
end function present_and_false
end module m


program test_sgn_real
use m, only: dp, sgn_real
implicit none
real(dp), parameter :: x(7) = [-1.0_dp, -0.5_dp, -0.25_dp, 0.0_dp, &
                               0.25_dp, 0.5_dp, 1.0_dp]
real(dp), parameter :: signs(7) = [-1.0_dp, -1.0_dp, -1.0_dp, 0.0_dp, &
                                   1.0_dp, 1.0_dp, 1.0_dp]
real(dp), parameter :: ramp(7) = [-1.0_dp, -1.0_dp, -0.5_dp, 0.0_dp, &
                                  0.5_dp, 1.0_dp, 1.0_dp]
real(dp), parameter :: deadband(7) = [-1.0_dp, -1.0_dp, 0.0_dp, 0.0_dp, &
                                      0.0_dp, 1.0_dp, 1.0_dp]
integer :: i

! Default sign, including exactly zero, for scalar and elemental array calls.
call check('default array', sgn_real(x), signs)
do i = 1, size(x)
   if (abs(sgn_real(x(i)) - signs(i)) > 1.0e-12_dp) then
      print *, 'FAIL: default scalar, element ', i
      error stop 1
   end if
end do

! An omitted SMOOTH defaults to a linear ramp inside the threshold.
! The input includes both threshold boundaries and values on either side.
call check('threshold, smooth omitted', sgn_real(x, 0.5_dp), ramp)
call check('threshold, smooth true', &
           sgn_real(x, thresh=0.5_dp, smooth=.true.), ramp)
call check('threshold, smooth false', &
           sgn_real(x, thresh=0.5_dp, smooth=.false.), deadband)

! SMOOTH alone does not change the result when THRESH is absent.
call check('no threshold, smooth true', sgn_real(x, smooth=.true.), signs)
call check('no threshold, smooth false', sgn_real(x, smooth=.false.), signs)
print *, 'All sgn_real tests passed.'

contains

subroutine check(label, actual, expected)
character(len=*), intent(in) :: label
real(dp), intent(in) :: actual(:), expected(:)
if (size(actual) /= size(expected)) then
   print *, 'FAIL: ', label, ': wrong size'
   error stop 1
end if
if (any(abs(actual - expected) > 1.0e-12_dp)) then
   print *, 'FAIL: ', label
   print *, 'actual:   ', actual
   print *, 'expected: ', expected
   error stop 1
end if
end subroutine check

end program test_sgn_real
