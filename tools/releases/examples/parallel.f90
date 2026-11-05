implicit none
integer :: i,j,a(10000),total,n,stride
integer :: matrix(100,100),totals(2)
integer,allocatable :: b(:),runtime_totals(:)
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
call fill(a,b)
if(b(n)/=n+1) stop 3
do concurrent(j=1:100,i=1:100)
   matrix(i,j)=i+j
end do
if(sum(matrix)/=1010000) stop 4
stride=-2
do concurrent(i=10000:1:stride)
   a(i-1)=i-1
end do
if(a(1)/=1 .or. a(9999)/=9999) stop 6
do concurrent(j=99:1:stride,i=1:99:2)
   matrix(i+1,j+1)=7
end do
if(matrix(2,2)/=7 .or. matrix(100,100)/=7 .or. matrix(1,1)/=2) stop 5
totals=7
do concurrent(i=1:10000) reduce(+:totals)
   totals=totals+[i,1]
end do
if(any(totals/=[50005007,10007])) stop 7
n=2
allocate(runtime_totals(n))
runtime_totals=7
do concurrent(i=1:10000) reduce(+:runtime_totals)
   runtime_totals=runtime_totals+1
end do
if(any(runtime_totals/=10007)) stop 8
do concurrent(i=1:10000) reduce(+:runtime_totals)
   runtime_totals(1:n)=runtime_totals(1:n)+a(1:n)
end do
if(any(runtime_totals/=[20007,30007])) stop 9
deallocate(runtime_totals)
deallocate(b)
print *,sum(a)
contains
subroutine fill(x,y)
integer,intent(in)::x(:)
integer,intent(out)::y(:)
integer::i
do concurrent(i=1:size(x)) shared(x,y) default(none)
   y(i)=x(i)+1
end do
end subroutine
end
