implicit none
integer::i
real(8)::a(9000)
do concurrent(i=1:9000)
 a(i)=probe(real(i,8)/9000d0)
end do
if(any(abs(a-1d0)>1d-12)) stop 1
print *, 'PASS'
contains
pure real(8) function probe(x)
real(8),intent(in)::x
probe=merge(erf(x)+erfc(x),gamma(1d0),x>0)
probe=probe+erfc_scaled(0d0)-1d0
probe=probe+bessel_jn(0,x)-bessel_j0(x)
probe=probe+bessel_yn(1,1d0)-bessel_y1(1d0)
end function
end
