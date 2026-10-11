program integer_literals
use, intrinsic :: iso_fortran_env, only: int8,int16,int32,int64
implicit none
integer,parameter::n=2_int64**3_int8, k=kind(1_int64+2_int8)
integer(int64)::wide,a(3)
integer::small
wide=10000000000_int64
small=3
a=[1_8,2_int64,3_INT64]
print *,127_1,32767_2,2147483647_4,9223372036854775807_8
print *,kind(1_1),kind(1_2),kind(1_4),kind(1_8),n,k
print *,kind(1_1+2_8),kind(1_8+2_1),kind(2_1**3_8)
print *,kind(small*wide),kind(wide*small),small*wide
print *,a,sum(a),kind(sum(a))
print *,huge(0_1),huge(0_2),huge(0_8),kind(huge(0_8))
print *,twice(7_8),twice(1_8+2_8)
print *,-2**2,(-2)**2,2**3**2,-2_8**2,2**(-3)
contains
pure integer(8) function twice(i)
integer(8),intent(in)::i
twice=i*2_8
end function twice
end program integer_literals
