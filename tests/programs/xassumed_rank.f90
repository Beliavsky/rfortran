program p
implicit none
real::x=2.0,a(3)=[1.,2.,3.],b(2,2)
b=4
call describe(x)
call describe(a)
call describe(b)
contains
subroutine describe(x)
real,intent(in)::x(..)
select rank(x)
rank(0)
print *,rank(x),x
rank(1)
print *,rank(x),size(x),sum(x)
rank(2)
print *,rank(x),shape(x),sum(x)
rank default
print *,rank(x)
end select
end subroutine
end program
