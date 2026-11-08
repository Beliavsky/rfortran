program particle_update
implicit none
integer, parameter :: n=9000, dp=kind(1.0d0)
type :: particle
   real(dp) :: x, velocity
   integer :: id
   logical :: active
end type
type(particle) :: particles(n)
integer :: i
real(dp), parameter :: dt=0.5_dp
do i=1,n
   particles(i)%x=real(i,dp)
   particles(i)%velocity=2.0_dp
   particles(i)%id=i
   particles(i)%active=mod(i,2)==0
end do
do concurrent (i=1:n)
   if (particles(i)%active) then
      particles(i)%x=particles(i)%x+dt*particles(i)%velocity
   end if
   particles(i)%id=particles(i)%id+1
end do
do i=1,n
   if (particles(i)%x/=real(i,dp)+merge(1.0_dp,0.0_dp,mod(i,2)==0)) stop 1
   if (particles(i)%id/=i+1) stop 2
end do
print *, 'PASS'
end program
