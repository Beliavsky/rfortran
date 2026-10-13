module kinds_mod
interface identify
module procedure ints,wide,reals,doubles,logicals,chars
end interface
contains
pure integer function ints(x) result(n)
integer,intent(in)::x(:)
n=10+size(x)
end function
pure integer function wide(x) result(n)
integer(8),intent(in)::x(:)
n=20+size(x)
end function
pure integer function reals(x) result(n)
real,intent(in)::x(:)
n=30+size(x)
end function
pure integer function doubles(x) result(n)
double precision,intent(in)::x(:)
n=40+size(x)
end function
pure integer function logicals(x) result(n)
logical,intent(in)::x(:)
n=50+size(x)
end function
pure integer function chars(x) result(n)
character(len=*),intent(in)::x(:)
n=60+size(x)
end function
end module
use kinds_mod
integer::i,j,v(2)
v=[2,3]
print *,identify([1,v,4]),identify([v,[5,6]])
print *,identify([((j,j=1,i),i=1,3)])
print *,identify([(2_8,i=1,3)]),identify([(i>1,i=1,3)])
print *,identify([real::]),identify([logical::])
end
