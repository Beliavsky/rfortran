module records
implicit none
type :: frame
integer,allocatable :: index(:)
real,allocatable :: values(:,:)
end type
contains
pure integer function nrow(x)
type(frame),intent(in)::x
nrow=size(x%index)
end function
elemental function subset(x,stride) result(y)
type(frame),intent(in)::x
integer,intent(in)::stride
type(frame)::y
y=frame(index=x%index(1:nrow(x):stride),values=x%values(1:nrow(x):stride,:))
end function
end module
program main
use records
implicit none
type(frame)::x,y
integer::stride
allocate(x%index(5),x%values(5,2))
x%index=[1,2,3,4,5]
x%values=reshape([1.,2.,3.,4.,5.,6.,7.,8.,9.,10.],[5,2])
stride=2
y=subset(x,stride)
print *,y%index
print *,y%values
end
