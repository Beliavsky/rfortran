implicit none
integer :: i,k
real :: a(500000),t,shift
do k=1,30
 shift=k*0.01
 do concurrent(i=1:500000) local(t) shared(a,shift) default(none)
  t=i*0.00001+shift
  a(i)=sin(t)+cos(t)*sqrt(t+1.0)
 end do
end do
print *,sum(a)
end
