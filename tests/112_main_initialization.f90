program main_initialization
use, intrinsic :: iso_fortran_env, only: int64
implicit none
integer,parameter::n=3
integer::i,a(-1:1)=[1,2,3], counter=2
integer,save::saved=9
integer(int64)::wide=10000000000_int64
real::r=1.25,values(n)=2.5
double precision::d=2.5d0
real(kind=8),parameter::b(2)=[1.5d0,2.5d0]
logical::flags(3)=[.true.,.false.,.true.],ready=.true.
character(5)::label='ok'
integer::matrix(2,2)=7,empty(0)=8
print *,a,counter,saved,wide
print *,r,values,d,b
print *,flags,ready
print *,'[',label,']'
print *,matrix,size(empty)
do i=1,3
counter=counter+1
a=a+1
end do
saved=saved+1
wide=wide+1
r=r+1.0
ready=.false.
label='done'
print *,a,counter,saved,wide,r,ready
print *,'[',label,']'
end program main_initialization
