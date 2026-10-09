module normal_mixture
   implicit none
   private
   integer, parameter, public :: dp = kind(1.0d0)
   real(dp), parameter :: pi = acos(-1.0_dp)
   public :: random_normal, simulate_mixture, fit_mixture_em
contains
   function random_normal() result(z)
      real(dp) :: z, u, v
      call random_number(u)
      call random_number(v)
      z = sqrt(-2.0_dp * log(max(u, tiny(u)))) * cos(2.0_dp*pi*v)
   end function random_normal

   subroutine simulate_mixture(x, weight, mu, sigma)
      real(dp), intent(out) :: x(:)
      real(dp), intent(in) :: weight(:), mu(:), sigma(:)
      real(dp) :: u, cumulative
      integer :: i, j, k
      k = size(weight)
      if (size(mu) /= k .or. size(sigma) /= k) error stop 'invalid parameter sizes'
      if (any(weight < 0.0_dp) .or. abs(sum(weight)-1.0_dp) > 1.0e-10_dp) &
         error stop 'invalid weights'
      if (any(sigma <= 0.0_dp)) error stop 'invalid standard deviations'
      do i = 1, size(x)
         call random_number(u)
         cumulative = 0.0_dp
         do j = 1, k-1
            cumulative = cumulative + weight(j)
            if (u < cumulative) exit
         end do
         if (j == k) then
            x(i) = mu(k) + sigma(k)*random_normal()
         else
            x(i) = mu(j) + sigma(j)*random_normal()
         end if
      end do
   end subroutine simulate_mixture

   subroutine fit_mixture_em(x, weight, mu, sigma, iterations, loglik, converged, max_iter, tol)
      real(dp), intent(in) :: x(:)
      real(dp), intent(inout) :: weight(:), mu(:), sigma(:)
      integer, intent(out) :: iterations
      real(dp), intent(out) :: loglik
      logical, intent(out) :: converged
      integer, intent(in), optional :: max_iter
      real(dp), intent(in), optional :: tol
      real(dp), allocatable :: nk(:), sumx(:), sumx2(:), logs(:), resp(:)
      real(dp) :: global_sd, var_floor, y, m, den, ll, old_ll, t, variance
      integer :: n, k, i, j, limit
      n = size(x)
      k = size(weight)
      if (n < 2 .or. k < 1) error stop 'invalid sample or component count'
      if (size(mu) /= k .or. size(sigma) /= k) error stop 'invalid parameter sizes'
      limit = 500
      if (present(max_iter)) limit = max_iter
      t = 1.0e-8_dp
      if (present(tol)) t = tol
      if (limit < 1 .or. t <= 0.0_dp) error stop 'invalid EM options'
      global_sd = sqrt(sum((x - sum(x)/real(n,dp))**2)/real(n,dp))
      if (global_sd <= 0.0_dp) error stop 'constant sample'
      var_floor = (1.0e-4_dp*global_sd)**2
      allocate(nk(k), sumx(k), sumx2(k), logs(k), resp(k))
      if (any(weight <= 0.0_dp) .or. any(sigma <= 0.0_dp)) &
         error stop 'positive starting weights and standard deviations required'
      weight = weight / sum(weight)
      old_ll = -huge(1.0_dp)
      converged = .false.
      do iterations = 1, limit
         nk = 0.0_dp
         sumx = 0.0_dp
         sumx2 = 0.0_dp
         ll = 0.0_dp
         do i = 1, n
            do j = 1, k
               y = (x(i)-mu(j))/sigma(j)
               logs(j) = log(weight(j)) - log(sigma(j)) - &
                  0.5_dp*log(2.0_dp*pi) - 0.5_dp*y*y
            end do
            m = maxval(logs)
            resp = exp(logs-m)
            den = sum(resp)
            resp = resp/den
            ll = ll + m + log(den)
            nk = nk + resp
            sumx = sumx + resp*x(i)
            sumx2 = sumx2 + resp*x(i)*x(i)
         end do
         if (iterations > 1) then
            if (abs(ll-old_ll) <= t*(1.0_dp+abs(old_ll))) then
               converged = .true.
               exit
            end if
         end if
         old_ll = ll
         do j = 1, k
            if (nk(j) <= 1.0e-12_dp) error stop 'empty mixture component'
            mu(j) = sumx(j)/nk(j)
            variance = max(var_floor, sumx2(j)/nk(j)-mu(j)**2)
            sigma(j) = sqrt(variance)
            weight(j) = nk(j)/real(n,dp)
         end do
      end do
      if (.not. converged) then
         iterations = limit
         ! Evaluate likelihood for the final updated parameters.
         ll = 0.0_dp
         do i = 1, n
            do j = 1, k
               y = (x(i)-mu(j))/sigma(j)
               logs(j) = log(weight(j))-log(sigma(j))-0.5_dp*log(2.0_dp*pi)-0.5_dp*y*y
            end do
            m = maxval(logs)
            ll = ll + m + log(sum(exp(logs-m)))
         end do
      end if
      loglik = ll
   end subroutine fit_mixture_em
end module normal_mixture

program test_normal_mixture
   use normal_mixture
   implicit none
   integer, parameter :: n = 1000, k = 2
   real(dp) :: x(n), weights(k), means(k), sds(k), loglik
   real(dp), parameter :: true_weights(k) = [0.35_dp, 0.65_dp]
   real(dp), parameter :: true_means(k) = [-2.0_dp, 3.0_dp]
   real(dp), parameter :: true_sds(k) = [0.8_dp, 1.5_dp]
   integer :: j, iterations
   logical :: converged

   call random_seed()
   call simulate_mixture(x, true_weights, true_means, true_sds)
   weights = 1.0_dp/real(k,dp)
   means = [-1.0_dp, 2.0_dp]
   sds = 2.0_dp
   call fit_mixture_em(x, weights, means, sds, iterations, loglik, converged)
   print '(a,i0)', 'Observations: ', n
   print '(a,i0)', 'EM iterations: ', iterations
   print '(a,l1)', 'Converged: ', converged
   print '(a,f16.3)', 'Log likelihood: ', loglik
   print '(a)', 'Comp    True wt   Est wt     True mean  Est mean   True sd    Est sd'
   do j = 1, k
      print '(i3,6f11.4)', j, true_weights(j), weights(j), true_means(j), &
         means(j), true_sds(j), sds(j)
   end do
end program test_normal_mixture
