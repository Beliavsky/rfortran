module m
implicit none
type item
integer::value
end type
contains
pure elemental integer function value_of(x) result(n)
type(item),intent(in)::x
n=x%value
end function
pure elemental function make_item(n) result(x)
integer,intent(in)::n
type(item)::x
x%value=n
end function
pure elemental function increment(x,n) result(y)
type(item),intent(in)::x
integer,intent(in)::n
type(item)::y
y=make_item(value_of(x)+n)
end function
end module
program main
use m
implicit none
type(item)::x,y
x=item(7)
y=increment(x,3)
print *,y%value
end
