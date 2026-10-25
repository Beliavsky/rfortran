program main
implicit none
character(len=8),allocatable::a(:),b(:),c(:)
allocate(a(2),b(3))
a=['red     ','blue    ']
b=['blue    ','green   ','red     ']
c=union_cols(a,b)
print *,size(c)
print '(a)',c
contains
function union_cols(a,b) result(c)
character(len=8),intent(in)::a(:),b(:)
character(len=8),allocatable::c(:),tmp(:)
integer::i,n
allocate(tmp(size(a)+size(b)))
n=size(a)
tmp(1:n)=a
do i=1,size(b)
 if (.not.any(tmp(1:n)==b(i))) then
  n=n+1
  tmp(n)=b(i)
 end if
end do
allocate(c(n))
c=tmp(1:n)
end function
end program
