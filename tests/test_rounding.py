"""FLOOR/CEILING in interpreter, native O0/O2, and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT

class Rounding(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/199_rounding.f90').read_text(),'rounding tests passed\n')

    def test_arrays_sections_and_pack(self):
        self.check("""real(8)::x(2,2)
integer::a(2,2)
logical::mask(2)
x=reshape([-1.2d0,1.2d0,-2.0d0,2.0d0],[2,2])
a=floor(x)
print *,a,ceiling(x(:,1)),floor(x(2:1:-1,1))
mask=[.true.,.false.]
print *,floor(pack(x(:,1),mask)),ceiling(pack(x(:,1),mask))
mask=.false.
print *,sum(floor(pack(x(:,1),mask))),sum(ceiling([real::]))
end
""",'-2 1 -2 2 -1 2 1 -2\n-2 -1\n0 0\n')

    def test_dynamic_arrays(self):
        self.check("""integer::n
real(8),allocatable::x(:,:)
integer,allocatable::a(:,:)
read(*,*) n
allocate(x(n,2),a(n,2))
x=-1.25d0
a=ceiling(x)
print *,sum(a),sum(floor(x(:,2)))
end
""",'-4 -4\n',data='2\n')

    def test_single_evaluation(self):
        self.check("""print *,floor(sample()),ceiling(sample())
contains
real function sample()
real::value
read(*,*) value
sample=value
end function
end
""",'-2 3\n',data='-1.2\n2.2\n')

    def test_rank_fifteen_and_empty(self):
        dims=','.join(['1']*15)
        self.check(f'real::x({dims})\nx=-1.2\nprint *,sum(floor(x)),sum(ceiling(x))\nend','-2 -1\n')

    def test_user_procedure_shadow(self):
        self.check("""print *,floor(2),ceiling(3)
contains
integer function floor(x)
integer,intent(in)::x
floor=x+10
end function
integer function ceiling(x)
integer,intent(in)::x
ceiling=x+20
end function
end
""",'12 23\n')

    def test_parameter_initializers(self):
        """NINT and INT of REAL and INTEGER constants in PARAMETER
        initializers, as `usual_days_in_year = nint(days_in_year)`."""
        self.check('integer, parameter :: dp = kind(1.0d0), n0 = 5\nreal(kind=dp), parameter :: days_in_year = 365.25_dp\ninteger, parameter :: usual = nint(days_in_year), a = int(2.7), b = floor(-2.5), c = ceiling(2.1d0)\ninteger, parameter :: e = nint(-2.5), f = nint(2.5), g = int(-2.7), h = int(n0+2), k = int(-3_8, kind=2)\ninteger(kind=8), parameter :: big = nint(days_in_year, kind=8)\ninteger :: v(usual - 360)\nprint *, usual, a, b, c, e, f, g, h, k, big, size(v)\nend\n',' 365 2 -3 3 -3 3 -2 7 -3 365 5\n')
        self.reject('integer, parameter :: v = nint(1.0e10)\nprint *, v\nend\n', 'NINT result is outside INTEGER range')

    def test_real_kind_rounding(self):
        self.check("""real::x
real(8)::y
read(*,*) x
read(*,*) y
print *,floor(1.99999999+x),ceiling(-1.99999999-x)
print *,floor(1.99999999d0+y),ceiling(-1.99999999d0-y)
end
""",'2 -2\n1 -1\n',data='0\n0\n')

    def test_integer_boundaries(self):
        self.check("""use iso_fortran_env,only:int8,int16,int32,int64
print *,floor(-128.0,int8),ceiling(127.0,int8)
print *,floor(-32768.0,int16),ceiling(32767.0,int16)
print *,floor(-2147483648.0d0,int32),ceiling(2147483647.0d0,int32)
print *,floor(-9223372036854775808.0d0,int64),ceiling(9223372036854774784.0d0,int64)
end
""",'-128 127\n-32768 32767\n-2147483648 2147483647\n-9223372036854775808 9223372036854774784\n')

    def test_range_errors(self):
        for name,data,kind in [('floor','128',1),('ceiling','-129',1),('floor','2147483648',4),('ceiling','9223372036854775808',8),('floor','NaN',4),('ceiling','Infinity',4)]:
            with self.subTest(name=name,data=data,kind=kind):
                self.check(f'real(8)::x\nread(*,*) x\nprint *,{name}(x,kind={kind})\nend',error=f'{name.upper()} result is outside INTEGER',data=data+'\n',reference=False)

    def test_validation(self):
        for name in ('floor','ceiling'):
            self.reject(f'print *,{name}(1)\nend','A must be REAL')
            self.reject(f'integer,parameter::n={name}(1)\nprint *,n\nend','A must be REAL')
            self.reject(f'print *,{name}(1.2,kind=3)\nend','supported INTEGER kind')
            self.reject(f'integer::k\nk=4\nprint *,{name}(1.2,kind=k)\nend','constant INTEGER')

if __name__=='__main__': unittest.main()
