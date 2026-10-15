"""Early RETURN in stepped loops and NINT: interpreter, O0/O2, and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT


class UtilCorrectness(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_reduced_util_fixture(self):
        self.check((ROOT/'tests/191_return_in_explicit_step.f90').read_text(),
                   '>,boy,,girl<\n><\n3 5 8\n')

    def test_subroutine_return_preserves_loop_variable(self):
        self.check('''program main
character(8)::text
integer::i
text='a,b,,,  '
call scan(text,i)
print *,i
print '(a)',text
print *,'caller continues'
contains
subroutine scan(s,i)
character(*),intent(inout)::s
integer,intent(out)::i
do i=len(s),1,-1
if(s(i:i)==' ') cycle
if(s(i:i)==',')then
s(i:i)=' '
else
return
end if
end do
end subroutine
end
''','3\na,b     \ncaller continues\n')

    def test_nested_loops_and_positive_steps(self):
        self.check('''program main
integer::i,j,n
call nested(i,j,n)
print *,i,j,n
call positive(i,n)
print *,i,n
contains
subroutine nested(i,j,n)
integer,intent(out)::i,j,n
n=0
do j=3,1,-1
do i=7,1,-2
if(i==7) cycle
if(j==2 .and. i==3) return
n=n+1
end do
n=n+10
end do
n=-999
end subroutine
subroutine positive(i,n)
integer,intent(out)::i,n
n=0
do i=1,9,2
select case(i)
case(5)
return
case default
n=n+1
end select
end do
n=-999
end subroutine
end
''','3 2 14\n5 2\n')

    def test_strip_elemental_repeated_and_empty(self):
        self.check('''program main
integer::i
print '(100(a,1x))',clean(['a,,','b,c',',,,','   '])
do i=1,2
print '(a)', '>'//trim(clean('a,b,, '))//'<'
end do
print '(a)', '>'//clean('')//'<'
contains
elemental function clean(text) result(s)
character(*),intent(in)::text
character(len(text))::s
integer::i
s=text
do i=len(text),1,-1
if(s(i:i)==' ') cycle
if(s(i:i)==',')then
s(i:i)=' '
else
return
end if
end do
end function
end
''','a b,c\n>a,b<\n>a,b<\n><\n')

    def test_return_precedes_step_overflow_check(self):
        # gfortran -fcheck=all rejects the maximum upper bound before entering
        # the loop. Check rfort's own increment guard here: RETURN avoids it.
        self.check('''program main
integer,parameter::wide=selected_int_kind(18)
integer(wide)::i
call stop_first(i)
print *,i
contains
subroutine stop_first(i)
integer(wide),intent(out)::i
integer(wide)::first,last
first=huge(i)
last=first
do i=first,last,1
return
end do
end subroutine
end
''','9223372036854775807\n',reference=False)

    def test_nint_positive_negative_ties(self):
        self.check('''program main
real::x(9)
integer::i
x=[-2.5,-2.49,-0.5,-0.49,0.0,0.49,0.5,2.49,2.5]
do i=1,9
print *,nint(x(i))
end do
end
''','-3\n-2\n-1\n0\n0\n0\n1\n2\n3\n')

    def test_nint_arrays_keywords_and_kind(self):
        self.check('''program main
integer,parameter::wide=selected_int_kind(18)
double precision::x(4)
integer(wide)::a(4)
x=[-2.5d0,-0.5d0,0.5d0,2.5d0]
a=nint(kind=wide,a=x)
print *,a,sum(nint(x))
print *,kind(nint(1.0)),kind(nint(a=1.0d0,kind=wide))
print *,nint(2147483648.5d0,kind=wide)
end
''','-3 -1 1 3 0\n4 8\n2147483649\n')

    def test_nint_large_integral_double_and_neighbors(self):
        self.check('''program main
integer,parameter::wide=selected_int_kind(18)
double precision::x(6)
integer::i
x=[4503599627370497.0d0,-4503599627370497.0d0, &
 0.49999999999999994d0,-0.49999999999999994d0, &
 2.5000000000000004d0,-2.5000000000000004d0]
do i=1,6
print *,nint(x(i),kind=wide)
end do
end
''','4503599627370497\n-4503599627370497\n0\n0\n3\n-3\n')

    def test_nint_user_function_and_scalar_actual(self):
        self.check('''program main
print *,rounded(2.5)
print *,nint(value(2.0))
call take(nint(2.5))
print '(i0)',nint(-2.5)
contains
integer function rounded(x)
real,intent(in)::x
rounded=nint(x)
end function
real function value(x)
real,intent(in)::x
value=x+0.5
end function
subroutine take(i)
integer,intent(in)::i
print *,i
end subroutine
end
''','3\n3\n3\n-3\n')
        self.check('''program main
print *,nint(2)
contains
integer function nint(x)
integer::x
nint=x+7
end function
end
''','9\n')

    def test_nint_rejects_invalid_arguments(self):
        for expression,message in [('nint(2)','NINT A must be REAL'),
                                   ('nint(.true.)','NINT A must be REAL'),
                                   ("nint('x')",'NINT A must be REAL'),
                                   ('nint(2.5,kind=16)','NINT KIND must be a supported')]:
            with self.subTest(expression=expression):
                self.reject(f'print *,{expression}\nend\n',message)


if __name__=='__main__':
    unittest.main(verbosity=2)
