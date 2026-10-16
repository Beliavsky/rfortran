"""Numeric MIN/MAX in array expressions, reductions, and runtime PACK results."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER


class NumericMinmax(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_normalize_and_array_intrinsics(self):
        self.check((ROOT/'tests/196_numeric_minmax.f90').read_text(),'minmax tests passed\n')

    def test_numeric_types_and_kinds(self):
        self.check('''integer,parameter::wide=selected_int_kind(18)
integer(wide)::a(2),b(2)
real::r(2)
double precision::d(2)
a=[3000000000_wide,-3000000000_wide]
b=max(a,2000000000_wide,-2000000000_wide)
print *,b,minval(min(a,b))
r=min([1.5,-2.5],0.5)
d=max([1.5d0,-2.5d0],0.5d0)
print *,r,d
end
''','3000000000 2000000000 -3000000000\n0.5 -2.5 1.5 0.5\n')

    def test_runtime_lengths_and_empty_arrays(self):
        self.check('''integer::a(3),n
read(*,*) n
a=[1,5,-3]
print *,min(pack(a,a>n),3)
print *,max([2,4],pack(a,a>n),0)
print *,sum(max(pack(a,a>100),0))
end
''','1 3\n2 5\n0\n',data='0\n')

    def test_multidimensional_arrays_and_sections(self):
        self.check('''integer::a(2,2),b(2,2)
a=reshape([1,5,-3,7],[2,2])
b=max(a,0)
print *,b
print *,min(a(:,2),2)
end
''','1 5 0 7\n-3 2\n')

    def test_runtime_shape_mismatch(self):
        self.check('''integer::a(3),n
read(*,*) n
a=[1,5,-3]
print *,max(pack(a,a>n),[1,2,3])
end
''',error='runtime PACK result shape does not conform',data='0\n',reference=False)

    def test_invalid_arguments(self):
        for expression,message in [
            ('min(a)','at least 2 arguments'),
            ('max(a,1.0)','same type and kind'),
            ('min(a,.true.)','numeric arguments'),
            ('max(a,[1,2])','nonconforming array argument shapes'),
        ]:
            with self.subTest(expression=expression):
                self.reject('integer::a(3)\na=[1,2,3]\nprint *,'+expression+'\nend\n',message)

    def test_equal_element_counts_do_not_make_shapes_conform(self):
        for declaration in ('b(3,2)','b(6)'):
            with self.subTest(declaration=declaration):
                self.reject('integer::a(2,3),'+declaration+'\na=1\nb=2\nprint *,min(a,b)\nend\n',
                            'nonconforming array argument shapes')

    def test_reduction_diagnostic_has_source_line(self):
        with tempfile.TemporaryDirectory(prefix='minmax-diagnostic-') as tmp:
            source=Path(tmp)/'example.f90'
            source.write_text('integer::a(2)\na=[1,2]\nprint *,sum(unsupported(a))\nend\n')
            result=subprocess.run([COMPILER,'run',source],capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('unsupported expression in numeric array reduction',result.stderr)
            self.assertIn('example.f90:3:',result.stderr)
            self.assertIn('print *,sum(unsupported(a))',result.stderr)

    def test_intrinsic_names_can_be_shadowed(self):
        self.check('''program main
integer::max(2)
max=[3,4]
print *,max(1),min(2,3)
contains
integer function min(a,b)
integer,intent(in)::a,b
min=a+b
end function min
end program main
''','3 5\n')


if __name__=='__main__':unittest.main()
