module impure_elementals
implicit none
contains
impure elemental subroutine show(i)
integer,intent(in)::i
print *,i,sqrt(real(i))
end subroutine show
elemental impure subroutine bump(i,delta)
integer,intent(inout)::i
integer,intent(in)::delta
i=i+delta
print *,i
end subroutine bump
impure elemental integer function twice(i)
integer,intent(in)::i
print *,i
twice=2*i
end function twice
elemental subroutine fill(i,default,option)
integer,intent(out)::i
integer,intent(in)::default
integer,intent(in),optional::option
if (present(option)) then
i=option
else
i=default
end if
end subroutine fill
end module impure_elementals
program test_impure
use impure_elementals
implicit none
integer::a(3),b(3),empty(0)
call show([2,3,4])
a=[1,2,3]
call bump(a,10)
print *,a
call bump(a(3:1:-1),[1,2,3])
print *,a
call fill(b(:),7)
print *,b
call fill(option=a,i=b,default=0)
print *,b
b=twice([4,2,3])
print *,b
call show(empty)
call show(9)
end program test_impure
