"""ISNAN and sorting checks in the interpreter, native O0/O2, and gfortran."""
import unittest
import shutil
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT
from run_ofort_compat import equivalent


class Isnan(unittest.TestCase):
    reject=test_deallocate.Deallocate.reject

    def check(self,program,expected=None,error=None,data='',reference=True):
        test_deallocate.Deallocate.check(self,program,expected,error,data,reference=False)
        # ISNAN is a GNU extension, excluded by the shared helper's -std=f2018.
        if reference and expected is not None and shutil.which('gfortran'):
            with tempfile.TemporaryDirectory(prefix='isnan-reference-') as tmp:
                source=Path(tmp)/'main.f90';exe=Path(tmp)/'reference.exe'
                source.write_text(program)
                result=subprocess.run(['gfortran','-std=gnu','-fcheck=all',source,'-o',exe],
                                      capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],input=data,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stderr,'')
                self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_special_values_both_real_kinds_and_purity(self):
        self.check((ROOT/'tests/197_isnan.f90').read_text(),'isnan tests passed\n',
                   data='NaN\nInfinity\n-Infinity\n-0.0\n')

    def test_scalar_expression_is_evaluated_once(self):
        self.check('''program main
logical::flag
flag=isnan(sample())
print *,flag
contains
real function sample()
real::value
read(*,*) value
sample=value
end function sample
end program main
''','F\n',data='1.0\n')

    def test_multidimensional_arrays_and_sections(self):
        self.check('''double precision::x(2,2),nan
logical::mask(2,2)
read(*,*) nan
x=reshape([nan,1.0d0,-2.0d0,nan],[2,2])
mask=isnan(x)
print *,mask,count(isnan(x))
print *,isnan(x(:,2)),isnan(x(2:1:-1,1))
print *,[isnan(x(1,1)),isnan(x(2,1))]
end
''','T F F T 2\nF T F T\nT F\n',data='NaN\n')

    def test_runtime_pack_and_empty_array(self):
        self.check('''double precision::x(3),nan
logical::mask(3)
read(*,*) nan
x=[1.0d0,nan,2.0d0]
mask=[.true.,.true.,.false.]
print *,isnan(pack(x,mask)),count(isnan(pack(x,mask)))
mask=.false.
print *,count(isnan(pack(x,mask))),any(isnan([real::]))
end
''','F T 1\n0 F\n',data='NaN\n')

    def test_runtime_sized_multidimensional_array(self):
        self.check('''integer::n
double precision::nan
double precision,allocatable::x(:,:)
logical,allocatable::flags(:,:)
read(*,*) n
read(*,*) nan
allocate(x(n,2),flags(n,2))
x=nan
x(1,1)=0.0d0
flags=isnan(x)
print *,count(flags),count(isnan(x(:,2:1:-1)))
end
''','3 3\n',data='2\nNaN\n')

    def test_higher_ranks(self):
        for rank in (3,15):
            with self.subTest(rank=rank):
                dims=','.join(['1']*rank)
                self.check('double precision::x('+dims+'),nan\nread(*,*) nan\nx=nan\n'
                           'print *,any(isnan(x)),count(isnan(x))\nend\n',
                           'T 1\n',data='NaN\n')

    def test_invalid_arguments(self):
        for expression in ('isnan(1)','isnan(.true.)',"isnan('text')"):
            with self.subTest(expression=expression):
                self.reject('print *,'+expression+'\nend\n','requires a REAL or DOUBLE PRECISION argument')
        self.reject('print *,isnan()\nend\n','expects 1 argument')
        self.reject('print *,isnan(1.0,2.0)\nend\n','expects 1 argument')
        self.reject('call isnan(1.0)\nend\n','function, not a subroutine')
        self.reject('print *,isnan(a=1.0)\nend\n','unknown keyword')
        self.reject('real::x\nprint *,isnan(x)\nend\n','uninitialized')

    def test_intrinsic_name_can_be_shadowed(self):
        self.check('''program main
print *,isnan(2)
contains
integer function isnan(i)
integer,intent(in)::i
isnan=i+1
end function isnan
end program main
''','3\n')


if __name__=='__main__':unittest.main()
