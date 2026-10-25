module m
implicit none
type box
character(3),allocatable::names(:)
real,allocatable::values(:,:)
end type
contains
function select_cols(x,idx) result(y)
type(box),intent(in)::x
integer,intent(in)::idx(:)
type(box)::y
y=box(names=x%names(idx),values=x%values(:,idx))
end function
end module
program main
use m
implicit none
type(box)::x,y
logical::mask(3)
allocate(x%names(3),x%values(2,3))
x%names=['one','two','tri']
x%values=reshape([1.,2.,3.,4.,5.,6.],[2,3])
mask=[.true.,.false.,.true.]
y=select_cols(x,pack([1,2,3],mask))
print '(2(a,1x))',y%names
print *,y%values
end
