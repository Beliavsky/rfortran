! Run with --parallel --parallel-report to verify native worker execution.
implicit none
integer,parameter::dp=kind(0d0)
integer::i,n
complex(dp)::t,z
complex(dp),allocatable::a(:),y(:)
n=9000
allocate(a(-2:n-3),y(n))
a=cmplx(3.0_dp,4.0_dp,kind=dp)
z=cmplx(1.0_dp,2.0_dp,kind=dp)
do concurrent(i=1:n)
y(i)=probe(i,a(i-3),z)
end do
do i=1,n
if(abs(real(y(i),dp)-real(4*i-2,dp))>0.001_dp) stop 1
if(abs(aimag(y(i))-real(2*i+6,dp))>0.001_dp) stop 2
end do
print *, 'PASS'
contains
pure function probe(i,x,z) result(r)
integer,intent(in)::i
complex(dp),intent(in)::x,z
complex(dp)::r,t
t=x+cmplx(real(i,dp),-real(i,dp),kind=dp)
r=t*z+conjg(t)
end function
end
