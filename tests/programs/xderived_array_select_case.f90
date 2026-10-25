module dates
implicit none
type date
integer::day
end type
type frame
type(date),allocatable::index(:)
end type
contains
function join(a,b) result(c)
type(date),intent(in)::a(:),b(:)
type(date),allocatable::c(:)
integer::i,n
allocate(c(size(a)+size(b)))
n=size(a)
do i=1,n
c(i)=a(i)
end do
do i=1,size(b)
c(n+i)=b(i)
end do
end function
function combine(self,other) result(out)
type(frame),intent(in)::self,other
type(frame)::out
type(date),allocatable::idx_out(:)
select case(size(self%index))
case(2)
idx_out=join(self%index,other%index)
case default
idx_out=other%index
end select
out%index=idx_out
end function
end module
program main
use dates
implicit none
type(frame)::a,b,c
a%index=[date(1),date(2)]
b%index=[date(3)]
c=combine(a,b)
print *,c%index%day
end
