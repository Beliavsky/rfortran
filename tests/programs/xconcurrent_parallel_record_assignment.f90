implicit none
integer,parameter :: n=9000
type :: motion
 real :: position(-1:1), velocity(-1:1)
end type
type :: particle
 type(motion) :: state
 integer :: count
 logical :: active
 complex :: z, v(2)
end type
type(particle) :: particles(n), work
integer :: i
do i=1,n
 particles(i)%state%position=real(i)
 particles(i)%state%velocity=[1.,2.,3.]
 particles(i)%count=i
 particles(i)%active=.true.
 particles(i)%z=cmplx(real(i),-2.)
 particles(i)%v=(3.,4.)
end do
work%count=-99
do concurrent(i=1:n) local(work)
 work=particles(i)
 work=work
 work%state%position=work%state%position+work%state%velocity
 work%count=work%count+1
 work%z=conjg(work%z)
 particles(i)=work
end do
if(work%count/=-99) stop 1
do i=1,n
 if(any(particles(i)%state%position/=real(i)+[1.,2.,3.])) stop 2
 if(particles(i)%count/=i+1) stop 3
 if(.not.particles(i)%active) stop 4
 if(particles(i)%z/=cmplx(real(i),2.)) stop 5
 if(any(particles(i)%v/=(3.,4.))) stop 6
end do
print *, 'PASS'
end
