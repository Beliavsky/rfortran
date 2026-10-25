module m
implicit none
type date
integer::year,month
end type
type frame
type(date),allocatable::index(:)
real,allocatable::values(:)
end type
contains
function copy(df) result(out)
type(frame),intent(in)::df
type(frame)::out
out=frame(index=df%index,values=df%values)
end function
end module
program main
use m
implicit none
type(frame)::a,b
a%index=[date(2020,1),date(2021,2)]
a%values=[3.,4.]
b=copy(a)
print *,b%index(1)%year,b%index(2)%month,b%values
end
