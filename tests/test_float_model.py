"""Floating-point model intrinsics in constant, interpreted and native code."""
import unittest
import test_deallocate


class FloatModel(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_scalar_kinds_keywords_and_purity(self):
        self.check('''use iso_fortran_env,only:real32,real64
implicit none
real(real32)::x
real(real64)::y
x=6.0_real32
y=-6.0_real64
print *,scale(i=-1,x=x),fraction(x=x),exponent(x=x),set_exponent(i=2,x=x)
print *,scale(y,2),fraction(y),exponent(y),set_exponent(y,-1)
print *,kind(scale(x,1)),kind(fraction(y)),kind(exponent(y)),kind(spacing(y))
print *,spacing(x)==epsilon(x)*4,rrspacing(x)==12582912.0_real32
print *,nearest(s=1.0_real64,x=x)>x,nearest(x=x,s=-1.0_real64)<x
print *,same(y)
contains
pure logical function same(z)
real(real64),intent(in)::z
same=scale(fraction(z),exponent(z))==z .and. rrspacing(z)>0.0_real64
end function
end
''','3 .75 3 3\n-24 -.75 3 -.375\n4 8 4 8\nT T\nT T\nT\n')

    def test_constant_expressions(self):
        self.check('''use iso_fortran_env,only:real32,real64,int64
implicit none
real(real32),parameter::a=scale(1.5_real32,3_int64)
real(real64),parameter::b=fraction(-6.0_real64),c=set_exponent(6.0_real64,-1)
integer,parameter::n=exponent(8.0_real64)
real(real64),parameter::s=spacing(1.0_real64),r=rrspacing(.75_real64)
real(real64),parameter::next=nearest(1.0_real64,1.0_real64)
real(real64),parameter::original(2)=[1.0_real64,2.0_real64]
real(real64),parameter::scaled(2)=scale(original,2)
integer,parameter::powers(2)=exponent(scaled)
real::values(n)
values=a
print *,a,b,c,n,size(values)
print *,s==epsilon(1.0_real64),r==6755399441055744.0_real64
print *,next-1.0_real64==s
print *,scaled,powers
end
''','12 -.75 .375 4 4\nT T\nT\n4 8 3 4\n')

    def test_elemental_arrays_and_scalar_expansion(self):
        self.check('''implicit none
real :: x(2,2),y(2,2)
integer :: powers(2,2),e(2,2)
x=reshape([1.0,2.0,4.0,8.0],[2,2])
powers=reshape([1,2,-1,-2],[2,2])
y=scale(x,powers)
e=exponent(x)
print *,y
print *,e
print *,fraction(x)
print *,set_exponent(x,1)
print *,all(nearest(x,1.0)>x),all(spacing(x)>0.0),all(rrspacing(x)>0.0)
print *,scale(1.0,powers)
end
''','2 8 2 2\n1 2 3 4\n.5 .5 .5 .5\n1 1 1 1\nT T T\n2 4 .5 .25\n')

    def test_runtime_arrays(self):
        self.check('''implicit none
real,allocatable::a(:),b(:)
integer::n
n=3
allocate(a(n),b(n))
a=[1.0,2.0,4.0]
b=scale(a,2)
print *,b
print *,exponent(a),fraction(a)
print *,all(nearest(a,-1.0)<a)
end
''','4 8 16\n1 2 3 .5 .5 .5\nT\n')

    def test_empty_and_rank_fifteen_arrays(self):
        self.check('''implicit none
real,parameter::empty(0)=[real::],scaled(0)=scale(empty,2)
real::x(1,1,1,1,1,1,1,1,1,1,1,1,1,1,2)
real::y(1,1,1,1,1,1,1,1,1,1,1,1,1,1,2)
x=1.5
y=scale(x,2)
print *,size(scaled),sum(scaled)
print *,sum(y),sum(exponent(x)),all(nearest(x,1.0)>x)
end
''','0 0\n12 2 T\n')

    def test_subnormals_zero_and_boundaries(self):
        self.check('''use iso_fortran_env,only:real32,real64
implicit none
real(real32)::x,t,z
real(real64)::y,u,w
t=tiny(1.0_real32)
u=tiny(1.0_real64)
x=scale(t,1-digits(t))
y=scale(u,1-digits(u))
z=0.0_real32
w=0.0_real64
print *,x==nearest(z,1.0),y==nearest(w,1.0_real64)
print *,scale(x,digits(t)-1)==t,scale(y,digits(u)-1)==u
print *,scale(x,-1)==z,scale(3*x,-1)==2*x
print *,fraction(x),exponent(x),fraction(y),exponent(y)
print *,spacing(x)==t,spacing(y)==u,spacing(z)==t,spacing(w)==u
print *,exponent(z),exponent(w),rrspacing(z),rrspacing(w)
print *,nearest(-x,1.0)==z,nearest(-y,1.0_real64)==w
print *,nearest(1.0,1.0)-1.0==epsilon(z)
end
''','T T\nT T\nT T\n.5 -148 .5 -1073\nT T T T\n0 0 0 0\nT T\nT\n')

    def test_invalid_calls_and_runtime_direction(self):
        for expr,message in [
            ('scale(1,2)','X must be REAL'),
            ('scale(1.0,2.0)','I must be INTEGER'),
            ('fraction(1)','X must be REAL'),
            ('exponent((1.0,2.0))','X must be REAL'),
            ('nearest(1.0,1)','S must be REAL'),
            ('nearest(1.0,0.0)','S must not be zero'),
            ('scale(1.0)','expects 2 arguments'),
        ]:
            with self.subTest(expr=expr):
                self.reject('print *,'+expr+'\nend\n',message)
        self.check('real::x,s\nx=1.0\ns=0.0\nprint *,nearest(x,s)\nend\n',
                   error='NEAREST S must not be zero')
        self.reject('real::x(2,3)\ninteger::i(3,2)\nx=1.0\ni=1\nprint *,scale(x,i)\nend\n',
                    'nonconforming array argument shapes')

    def test_overflow_infinities_and_signed_zero(self):
        self.check('''use iso_fortran_env,only:real32,real64,int64
implicit none
real(real32)::x,z,inf,neg
real(real64)::y,w,big,ninf
integer(int64)::large
x=huge(1.0_real32)
y=huge(1.0_real64)
inf=scale(x,1)
big=scale(y,1)
neg=-inf
ninf=-big
z=-0.0_real32
w=-0.0_real64
large=huge(large)
print *,inf>x,big>y,nearest(inf,-1.0)==x,nearest(big,-1.0_real64)==y
print *,nearest(neg,1.0)==-x,nearest(ninf,1.0_real64)==-y
print *,nearest(inf,1.0)==inf,nearest(ninf,-1.0_real64)==ninf
print *,isnan(fraction(inf)),isnan(spacing(big)),isnan(rrspacing(inf))
print *,scale(z,1)==z,set_exponent(w,1)==w
print *,scale(1.0,-large)==0.0,scale(-1.0_real64,large)<-y
print *,nearest(-nearest(0.0_real64,1.0_real64),1.0_real64)==w
end
''','T T T T\nT T\nT T\nT T T\nT T\nT T\nT\n',reference=False)

    def test_evaluation_once_and_user_procedure_shadowing(self):
        self.check('''implicit none
real::x
x=nearest(next(),1.0)
print *,x>1.0,x<2.0
x=scale(next(),1)
print *,x
print *,next()
contains
function next() result(x)
real::x
integer,save::n=0
n=n+1
x=real(n)
end function
function scale(x,i) result(y)
real,intent(in)::x
integer,intent(in)::i
real::y
y=x+real(i)
end function
end
''','T T\n3\n3\n')


if __name__ == '__main__':
    unittest.main()
