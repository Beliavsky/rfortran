"""INTEGER kinds, checked arithmetic, constant overflow and unchecked wrapping."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

class IntegerOverflow(unittest.TestCase):
    def run_source(self,source,expected=None,error=None,compile_error=False,unchecked=False,parallel=False):
        with tempfile.TemporaryDirectory(prefix='rfort-integer-') as tmp:
            path=Path(tmp)/'case.f90'; path.write_text(source)
            for opt in ('0','2'):
                with self.subTest(optimization=opt):
                    exe=Path(tmp)/'case.exe'
                    command=[str(COMPILER),'-O',opt,str(path),'-o',str(exe)]
                    if unchecked: command.append('--nocheck')
                    if parallel: command.extend(['--parallel','--parallel-report'])
                    build=subprocess.run(command,cwd=tmp,capture_output=True,text=True,timeout=90)
                    if compile_error:
                        self.assertNotEqual(build.returncode,0)
                        self.assertIn(error,build.stderr)
                        self.assertIn('case.f90:',build.stderr)
                        continue
                    self.assertEqual(build.returncode,0,build.stderr)
                    if parallel: self.assertIn('native parallel worker emitted',build.stderr)
                    env=os.environ.copy()
                    if parallel: env['RFORT_NUM_THREADS']='2'
                    result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=20)
                    if error:
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(error,result.stderr)
                        self.assertRegex(result.stderr,r'line [1-9][0-9]*')
                    else:
                        self.assertEqual(result.returncode,0,result.stderr)
                        self.assertEqual(result.stdout.split(),expected.split())

    def test_default_power_overflow(self):
        self.run_source('implicit none\ninteger::i,j\ndo i=9,12\nj=10**i\nprint *,i,j\nend do\nend\n',error='INTEGER(kind=4) overflow')

    def test_parallel_worker_overflow(self):
        self.run_source('implicit none\ninteger::i,n,a(8)\nn=huge(n)\ndo concurrent(i=1:8)\na(i)=n+i\nend do\nprint *,a\nend\n',error='INTEGER(kind=4) overflow',parallel=True)

    def test_explicit_int64_power(self):
        self.run_source('''use iso_fortran_env,only:int64
implicit none
integer::i
integer(int64)::j
do i=9,12
j=10_int64**i
print *,i,j
end do
end
''',expected='9 1000000000 10 10000000000 11 100000000000 12 1000000000000')

    def test_destination_does_not_widen_expression(self):
        self.run_source('use iso_fortran_env,only:int64\nimplicit none\ninteger::i\ninteger(int64)::j\ni=10\nj=10**i\nprint *,j\nend\n',error='INTEGER(kind=4) overflow')

    def test_wider_exponent_widens_the_result(self):
        # INTEGER ** INTEGER has the kind of the operand with the greater range.
        self.run_source('use iso_fortran_env,only:int64\nimplicit none\ninteger(int64)::i,j\ni=10\nj=10**i\nprint *,j\nend\n',expected='10000000000')

    def test_arithmetic_overflow_in_each_kind(self):
        for k in (1,2,4,8):
            for op,setup,expr in [('add','x=huge(x)\ny=1','x+y'),('sub','x=-huge(x)-1\ny=1','x-y'),('mul','x=huge(x)\ny=2','x*y'),('neg','x=-huge(x)-1\ny=1','-x'),('div','x=-huge(x)-1\ny=-1','x/y'),('abs','x=-huge(x)-1\ny=1','abs(x)')]:
                with self.subTest(kind=k,op=op):
                    self.run_source(f'implicit none\ninteger(kind={k})::x,y\n{setup}\nprint *,{expr}\nend\n',error=f'INTEGER(kind={k}) overflow')

    def test_narrow_assignment_and_int(self):
        for expression in ('x','int(x,kind=1)'):
            self.run_source(f'implicit none\ninteger::x\ninteger(kind=1)::y\nx=128\ny={expression}\nprint *,y\nend\n',error='INTEGER(kind=1) overflow')

    def test_fixed_and_allocatable_array_arithmetic(self):
        for declaration,allocation in [('integer::a(2)',''),('integer,allocatable::a(:)','allocate(a(2))')]:
            self.run_source(f'implicit none\n{declaration}\ninteger::i\n{allocation}\na=huge(0)\ni=1\nprint *,a(i)+a(i+1)\nend\n',error='INTEGER(kind=4) overflow')

    def test_fixed_and_allocatable_array_narrow_assignment(self):
        for declaration,allocation in [('integer(kind=1)::a(2)',''),('integer(kind=1),allocatable::a(:)','allocate(a(2))')]:
            self.run_source(f'implicit none\n{declaration}\ninteger::i,x\n{allocation}\nx=128\ni=1\na(i)=x\nprint *,a(i)\nend\n',error='INTEGER(kind=1) overflow')

    def test_pointer_arithmetic_and_assignment(self):
        self.run_source('implicit none\ninteger,target::x\ninteger,pointer::p\nx=huge(x)\np=>x\nprint *,p+1\nend\n',error='INTEGER(kind=4) overflow')
        self.run_source('implicit none\ninteger(kind=1),target::x\ninteger(kind=1),pointer::p\ninteger::n\np=>x\nn=128\np=n\nprint *,x\nend\n',error='INTEGER(kind=1) overflow')
        self.run_source('implicit none\ninteger(kind=8),target::x\ninteger(kind=8),pointer::p\nx=3000000000_8\np=>x\nprint *,p+1\nend\n',expected='3000000001')

    def test_real_conversion(self):
        self.run_source('implicit none\nreal(kind=8)::x\ninteger::i\nx=3d10\ni=int(x)\nprint *,i\nend\n',error='INTEGER(kind=4) overflow')
        self.run_source('implicit none\nreal(kind=8)::x\ninteger(kind=1)::i\nx=-128.9d0\ni=int(x,kind=1)\nprint *,i\nend\n',expected='-128')

    def test_integer_zero_divisors(self):
        for expr in ('1/z','z**n'):
            self.run_source(f'implicit none\ninteger::z,n,x\nz=0\nn=-1\nx={expr}\nprint *,x\nend\n',error='division by zero')

    def test_constant_overflow(self):
        for declaration,expr in [('integer,parameter::n','10**10'),('integer(kind=8),parameter::n','10**10'),('integer(kind=8),parameter::n','int(10**10,kind=8)'),('integer,parameter::n','2147483647+1'),('integer(kind=1),parameter::n','int(128,kind=1)')]:
            with self.subTest(expr=expr):
                self.run_source(f'implicit none\n{declaration}={expr}\nprint *,n\nend\n',error='overflow',compile_error=True)
        self.run_source('implicit none\ninteger(kind=8)::n\nn=10**10\nprint *,n\nend\n',error='overflow',compile_error=True)
        self.run_source('implicit none\ninteger::n\nn=int(3d10)\nprint *,n\nend\n',error='overflow',compile_error=True)

    def test_valid_boundaries_and_negative_exponents(self):
        self.run_source('''use iso_fortran_env,only:int64
implicit none
integer::e,b
integer(int64)::wide
e=30
b=2
print *,b**e
e=62
wide=2_int64**e
print *,wide
e=-3
print *,2**e,(-1)**e,1**e
end
''',expected='1073741824 4611686018427387904 0 -1 1')

    def test_mixed_integer_kinds_promote_operands(self):
        self.run_source('implicit none\ninteger(kind=1)::small\ninteger(kind=8)::wide\nsmall=127_1\nwide=1_8\nprint *,small+1,small+wide,huge(0)+wide\nend\n',expected='128 128 2147483648')

    def test_unchecked_wraps_in_declared_kind(self):
        self.run_source('implicit none\ninteger::i,j\ni=10\nj=10**i\nprint *,j\nend\n',expected='1410065408',unchecked=True)
        self.run_source('implicit none\ninteger(kind=1)::x,y\nx=127\ny=1\nx=x+y\nprint *,x\nend\n',expected='-128',unchecked=True)

    def test_boz_conversions_are_bit_interpretation(self):
        self.run_source("implicit none\ninteger(kind=1),parameter::a=int(z'ff',kind=1)\ninteger,parameter::b=int(z'ffffffff')\nprint *,a,b\nend\n",expected='-1 -1')

if __name__=='__main__': unittest.main()
