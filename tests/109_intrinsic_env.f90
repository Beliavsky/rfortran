module numeric_kinds
use, intrinsic :: iso_fortran_env, only: int32, int64, real64
implicit none
private
integer, parameter, public :: wide_kind=int64, double_kind=real64
end module numeric_kinds
program intrinsic_env
use :: numeric_kinds, only: wide_kind, double_kind
use, intrinsic :: iso_fortran_env, only: int8, int16, int32, int64, real32, real64
implicit none
integer(int8)::small
integer(int16)::medium
integer(int32)::normal
integer(wide_kind)::wide
real(real32)::single
real(double_kind)::d
small=12
medium=1234
normal=123456
wide=huge(wide)
single=1.25
d=2.5d0
print *,int8,int16,int32,int64,real32,real64
print *,kind(small),kind(medium),kind(normal),kind(wide),kind(single),kind(d)
print *,small,medium,normal,wide
print *,single,d
end program intrinsic_env
