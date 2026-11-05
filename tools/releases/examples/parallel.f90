implicit none
integer :: i,a(10000),total,n
integer,allocatable :: b(:)
do concurrent(i=1:10000) shared(a) default(none)
   a(i)=i
end do
total=7
do concurrent(i=1:10000) shared(a) reduce(+:total) default(none)
   total=total+a(i)
end do
if(total/=50005007) stop 1
n=10000
allocate(b(n))
total=7
do concurrent(i=1:n) shared(a,b) reduce(+:total) default(none)
   b(i)=a(i)
   total=total+b(i)
end do
if(total/=50005007 .or. b(n)/=n) stop 2
deallocate(b)
print *,sum(a)
end
