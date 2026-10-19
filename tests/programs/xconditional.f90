program test_conditional
implicit none
integer :: x=1, y=2, z=3, edge=5, mode=0, value
value=(x>0 ? x : y>0 ? y : z)
print *,value
call add((x>0 ? x : y>0 ? y : z), (edge>0 ? edge : mode==3 ? 1 : .nil.))
print *,x,y,z
call add((.false. ? x : y), (.false. ? edge : .nil.))
print *,x,y,z
contains
subroutine add(x,amount)
integer,intent(inout)::x
integer,intent(in),optional::amount
if(present(amount)) then
    x=x+amount
else
    x=x+10
end if
end subroutine
end program
