
module normal_random
   implicit none
   private
   integer, parameter, public :: dp = kind(1.0d0)
   public :: random_normal

contains

   ! Generate a standard normal random variate using Box-Muller.
   function random_normal() result(z)
      real(dp) :: z
      real(dp) :: u1, u2
      real(dp), parameter :: twopi = 2.0_dp * acos(-1.0_dp)

      call random_number(u1)
      call random_number(u2)
      u1 = max(u1, tiny(1.0_dp))
      z = sqrt(-2.0_dp * log(u1)) * cos(twopi * u2)
   end function random_normal

end module normal_random

program test_normal_random
   use normal_random
   implicit none

   integer, parameter :: n = 1000
   real(dp) :: x(n)
   real(dp) :: xmean, xsd, kurt, excess_kurt
   real(dp) :: m2, m4
   integer :: i

   call random_seed()

   do i = 1, n
      x(i) = random_normal()
   end do

   xmean = sum(x) / real(n, dp)
   m2 = sum((x - xmean)**2) / real(n, dp)
   m4 = sum((x - xmean)**4) / real(n, dp)

   xsd = sqrt(m2)
   kurt = m4 / m2**2
   excess_kurt = kurt - 3.0_dp

   print *, "Number of observations:", n
   print *, "Mean:              ", xmean
   print *, "Standard deviation:", xsd
   print *, "Kurtosis:          ", kurt
   print *, "Excess kurtosis:   ", excess_kurt

end program test_normal_random
