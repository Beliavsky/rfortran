module m
implicit none
type :: row
integer, allocatable :: x(:)
character(len=4), allocatable :: label(:)
end type
contains
elemental function scaled(a,n) result(b)
type(row),intent(in)::a
integer,intent(in)::n
type(row)::b
b=a
b%x=b%x*n
end function
end module
use m
implicit none
type(row)::a,b(2)
allocate(a%x(2),a%label(1))
a%x=[2,3]
a%label='name'
b=scaled(a,[2,3])
b(1)%x(1)=99
print *,b(1)%x,b(2)%x,a%x
print '(a)',b(1)%label,b(2)%label
end
