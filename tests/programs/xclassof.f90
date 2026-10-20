module m
implicit none
type::base
integer::value
end type
type,extends(base)::child
integer::extra
end type
contains
subroutine work(a,b)
class(base),intent(in)::a
classof(a),intent(in)::b
typeof(a)::copy
copy%value=a%value+b%value
print *,copy%value
end subroutine
end module
program main
use m
implicit none
type(child)::a,b
a%value=3;a%extra=10;b%value=4;b%extra=20
call work(a,b)
end
