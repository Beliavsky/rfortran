implicit none
integer,parameter::dp=selected_real_kind(r=307,p=15)
integer,parameter::rk=selected_real_kind(r=307)
integer,parameter::ik=selected_int_kind(r=9)
real::x
character(4)::s
integer::a(2,2),b(2,2)
x=2.75
s='a   '
print *,int(a=x),real(a=3),dble(a=4)
print *,sqrt(x=4.0),atan2(x=1.0,y=0.0)
print *,min(a3=2,a1=7,a2=4),max(a2=4,a3=9,a1=2),max(a1=2,a2=1,a4=3)
print *,abs(a=-3),mod(p=4,a=11),modulo(p=4,a=-1)
print *,dp,rk,ik,kind(x=1.0),digits(x=1),radix(x=x)
print *,len(string=s),len_trim(string=s)
print *,len(string='xy',kind=8),len_trim(string='xy  ',kind=8)
print *,ichar(c='A'),iachar(c='B',kind=4)
print *,char(i=65,kind=1)//achar(i=66)
a=reshape(shape=[2,2],source=[1,2,3,4])
b=transpose(matrix=a)
print *,sum(array=b),sum(array=pack(mask=a>2,array=a))
end
