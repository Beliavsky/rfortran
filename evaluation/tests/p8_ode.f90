module rk
implicit none
integer, parameter :: dp = kind(1.0d0)
abstract interface
  function f_i(t, y) result(dy)
    import dp
    real(dp), intent(in) :: t, y(:)
    real(dp) :: dy(size(y))
  end function
end interface
contains
subroutine rk4(f, t, y, h)
  procedure(f_i) :: f
  real(dp), intent(inout) :: t, y(:)
  real(dp), intent(in) :: h
  real(dp), dimension(size(y)) :: k1, k2, k3, k4
  k1 = f(t, y)
  k2 = f(t+h/2, y+h/2*k1)
  k3 = f(t+h/2, y+h/2*k2)
  k4 = f(t+h, y+h*k3)
  y = y + h/6*(k1+2*k2+2*k3+k4)
  t = t + h
end subroutine
function osc(t, y) result(dy)
  real(dp), intent(in) :: t, y(:)
  real(dp) :: dy(size(y))
  dy(1) = y(2); dy(2) = -y(1)
end function
end module
program p8
use rk
implicit none
real(dp) :: t, y(2)
integer :: i
t = 0; y = [1.0_dp, 0.0_dp]
do i = 1, 100
  call rk4(osc, t, y, 0.01_dp)
end do
print '(3f12.6)', t, y
end program
