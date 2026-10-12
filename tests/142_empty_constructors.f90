program main
integer,parameter::dp=kind(1.0d0),ik=8
integer,parameter::n=size([real(kind=dp)::]),k=kind([real(kind=dp)::])
real(dp)::a(0)
a=[real(kind=dp)::]
print *,size(a),n,size([integer::]),size([logical::]),size([double precision::])
print *,k,kind([integer(kind=ik)::]),kind([logical::]),kind([character::])
print *,len([character(len=3)::]),size([character(len=3)::])
print *,sum([integer::]),product([integer::]),sum([real(kind=dp)::]),product([real(kind=dp)::])
print *,all([logical::]),any([logical::]),count([logical::])
print *,7
end program main
