program rounding
implicit none
integer,parameter::small=selected_int_kind(2),wide=selected_int_kind(18)
integer,parameter::lo=floor(-1.25),hi=ceiling(1.25)
real::x(5)
real(8)::y
if(floor(1.99999999)/=2 .or. ceiling(-1.99999999)/=-2) stop 12
if(lo/=-2 .or. hi/=2) stop 10
x=[-1.75,-1.0,0.0,1.0,1.75]
if(any(floor(x)/=[-2,-1,0,1,1])) stop 1
if(any(ceiling(x)/=[-1,-1,0,1,2])) stop 2
y=-2147483649.25d0
if(floor(kind=wide,a=y)/=-2147483650_wide) stop 3
if(ceiling(y,wide)/=-2147483649_wide) stop 4
if(kind(floor(1.2,kind=small))/=small) stop 5
if(kind(ceiling(1.2,kind=wide))/=wide) stop 6
if(kind(floor(1.2,kind=selected_int_kind(4)))/=selected_int_kind(4)) stop 11
if(f(1.25)/=3) stop 7
if(floor(-128.0,small)/=-128) stop 8
if(ceiling(127.0,small)/=127) stop 9
print *, 'rounding tests passed'
contains
pure integer function f(a)
real,intent(in)::a
f=floor(a)+ceiling(a)
end function
end program
