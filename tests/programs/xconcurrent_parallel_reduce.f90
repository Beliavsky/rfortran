implicit none
integer :: i,k,total,lo,hi
real :: product,rsum
logical :: every,any_value,equivalent,parity
do k=1,3
 total=7
 product=2.0
 lo=20000
 hi=-3
 rsum=7.0
 every=.true.
 any_value=.false.
 equivalent=.false.
 parity=.true.
 do concurrent(i=1:10000) reduce(+:total,rsum) reduce(*:product) &
    reduce(min:lo) reduce(max:hi) reduce(.and.:every) &
    reduce(.or.:any_value) reduce(.eqv.:equivalent) reduce(.neqv.:parity)
    total=total+i
    rsum=rsum+1.0
    product=product*1.0
    lo=min(lo,i)
    hi=max(hi,i)
    every=every .and. i>0
    any_value=any_value .or. i==9999
    equivalent=equivalent .eqv. i/=9999
    parity=parity .neqv. i==9999
 end do
end do
print *,total,product,lo,hi,every,any_value,equivalent,parity,rsum
end
