implicit none
integer,parameter :: n=9000, dp=kind(1d0)
type :: state_type
 complex(dp) :: amplitude, vector(3)
end type
type :: particle
 type(state_type) :: state
end type
type(particle) :: particles(n)
integer :: i,j
do i=1,n
 particles(i)%state%amplitude=cmplx(real(i,dp),2.0_dp,kind=dp)
 particles(i)%state%vector=[(1.0_dp,2.0_dp),(3.0_dp,4.0_dp),(5.0_dp,6.0_dp)]
end do
do concurrent(i=1:n)
 particles(i)%state%amplitude=particles(i)%state%amplitude*(0.0_dp,1.0_dp)
 particles(i)%state%vector=conjg(particles(i)%state%vector)*(0.0_dp,1.0_dp)
end do
do i=1,n
 if(abs(particles(i)%state%amplitude-cmplx(-2.0_dp,real(i,dp),kind=dp))>1e-8_dp) stop 1
 do j=1,3
  if(abs(particles(i)%state%vector(j)-cmplx(real(2*j,dp),real(2*j-1,dp),kind=dp))>1e-8_dp) stop 2
 end do
end do
print *, 'PASS'
end
