program nested_particles
implicit none
integer, parameter :: n=9000
type :: state_type
   real :: position(3), velocity(3)
end type
type :: particle
   type(state_type) :: state
end type
type(particle) :: particles(n)
integer :: i,j
do i=1,n
   particles(i)%state%position=[real(i),real(i+1),real(i+2)]
   particles(i)%state%velocity=[2.0,4.0,6.0]
end do
do concurrent (i=1:n)
   particles(i)%state%position=particles(i)%state%position+0.5*particles(i)%state%velocity
end do
do i=1,n
   do j=1,3
      if (particles(i)%state%position(j)/=real(i+2*j-1)) stop 1
   end do
end do
print *, 'PASS'
end program
