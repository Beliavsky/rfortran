implicit none
integer,parameter :: n=9000
type :: state_type
 real :: position(3), velocity(3)
 integer :: count=2
end type
type :: workspace
 type(state_type) :: state
 logical :: active=.true.
end type
type(workspace) :: work
real :: values(n)
integer :: i
work%state%position=[10.0,20.0,30.0]
work%state%velocity=[1.0,2.0,3.0]
work%state%count=7
work%active=.false.
do concurrent(i=1:n) local(work)
 work%state%position=real(i)
 work%state%velocity=[1.0,2.0,3.0]
 values(i)=sum(work%state%position+work%state%velocity)+real(work%state%count)
 if(.not.work%active) values(i)=-999.0
end do
do i=1,n
 if(values(i)/=real(3*i+8)) stop 2
end do
if(work%state%count/=7 .or. work%active) stop 3
if(any(work%state%position/=[10.0,20.0,30.0])) stop 4
do concurrent(i=1:n) local_init(work)
 work%state%position=work%state%position+real(i)
 work%state%count=work%state%count+1
 values(i)=sum(work%state%position)+real(work%state%count)
end do
do i=1,n
 if(values(i)/=real(3*i+68)) stop 5
end do
if(work%state%count/=7 .or. work%active) stop 6
if(any(work%state%position/=[10.0,20.0,30.0])) stop 7
print *, 'PASS'
end
