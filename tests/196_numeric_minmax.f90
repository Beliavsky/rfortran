program test_numeric_minmax
implicit none
integer,parameter::dp=selected_real_kind(15)
real(dp)::a(3),z(2)
integer::i(3)
a=normalize([1.0_dp,-2.0_dp,1.0_dp])
if (any(abs(a-[0.25_dp,-0.5_dp,0.25_dp])>1.0e-12_dp)) error stop 'normalize'
z=normalize([0.0_dp,0.0_dp])
if (any(z/=0.0_dp)) error stop 'zero normalize'
i=min([1,5,-3],3)
if (any(i/=[1,3,-3])) error stop 'min broadcast'
i=min(a3=3,a1=[1,5,-3],a2=[2,4,-2])
if (any(i/=[1,3,-3])) error stop 'min keyword arguments'
i=max([1,5,-3],[2,4,-2],0)
if (any(i/=[2,5,0])) error stop 'max arrays'
if (sum(min([1,5,-3],3))/=1) error stop 'nested reduction'
print *, 'minmax tests passed'
contains
function normalize(x) result(y)
real(dp),intent(in)::x(:)
real(dp)::y(size(x))
y=x/max(sum(abs(x)),1.0e-10_dp)
end function normalize
end program test_numeric_minmax
