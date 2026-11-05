implicit none
integer :: i,a(10000),total
do concurrent(i=1:10000) shared(a) default(none)
   a(i)=i
end do
total=7
do concurrent(i=1:10000) shared(a) reduce(+:total) default(none)
   total=total+a(i)
end do
if(total/=50005007) stop 1
print *,sum(a)
end
