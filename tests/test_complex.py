"""COMPLEX lowering in the interpreter, native O0/O2, and gfortran."""
from pathlib import Path
import subprocess
import re
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent
import test_character_comparisons

class ComplexTests(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def check(self, source, expected, reference=True):
        with tempfile.TemporaryDirectory(prefix="complex-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text(source, encoding="utf-8")
            if reference:
                self.compare(path, expected, directory)
            else:
                for command in build_commands(path,directory):
                    with self.subTest(command=command):
                        result=subprocess.run(command,capture_output=True,text=True,encoding="utf-8",timeout=30)
                        self.assertEqual(result.returncode,0,result.stderr)
                        self.assertEqual(result.stderr,"")
                        self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_scalar(self):
        self.check("""implicit none
complex::z,w
z=(3.,4.)
w=cmplx(1.,-2.)
print *,real(z),aimag(z),abs(z)
print *,real(z+w),aimag(z+w),real(z*w),aimag(z*w)
print *,abs(z/z-(1.,0.))<1.e-6
print *,abs(z**2-(-7.,24.))<1.e-6
print *,abs(z**(-1)-(.12,-.16))<1.e-6
print *,conjg(z)
end
""", "3 4 5\n4 2 11 -2\nT\nT\nT\n(3,-4)\n")

    def test_arrays(self):
        self.check("""implicit none
complex::z(3)
z%re=3.
z%im=4.
print *,z
print *,abs(z)
print *,real(sum(z)),aimag(sum(z))
print *,real(dot_product(z,z)),aimag(dot_product(z,z))
end
""", "(3,4) (3,4) (3,4)\n5 5 5\n9 12\n75 0\n")

    def test_kind(self):
        self.check("""use iso_fortran_env,only:real32,real64
implicit none
complex(real64)::z
z=cmplx(1.000000000001_real64,2._real64,kind=real64)
print *,kind(z),kind(real(z)),kind(aimag(z)),kind(cmplx(z))
print *,abs(real(z)-1.000000000001_real64)<1.e-14_real64
end
""", "8 8 8 4\nT\n")

    def test_constants_and_functions(self):
        self.check("""implicit none
complex,parameter::z=(1.,2.),w=conjg(z)*(2.,0.)
complex::a
integer::calls
calls=0
a=twice(z,calls)
print *,a,w,calls
contains
function twice(x,counter) result(y)
complex,intent(in)::x
complex::y
integer,intent(inout)::counter
counter=counter+1
y=2*x
end function
end
""", "(2,4) (2,-4) 1\n")

    def test_intrinsic_math(self):
        self.check("""implicit none
complex(8)::z,w
z=(.3d0,.4d0)
w=sqrt(z)
print *,abs(w*w-z)<1.d-12
print *,abs(exp(log(z))-z)<1.d-12
print *,abs(sin(z)**2+cos(z)**2-(1.d0,0.d0))<1.d-12
print *,abs(sinh(z)/cosh(z)-tanh(z))<1.d-12
print *,abs(sin(z)/cos(z)-tan(z))<1.d-12
end
""", "T\nT\nT\nT\nT\n")

    def test_formatted_output_and_loop(self):
        self.check("""implicit none
complex::z
integer::i
z=(1.,2.)
do i=1,2
z=z*z
print '(2f10.2)',z
end do
end
""", "-3.00 4.00\n-7.00 -24.00\n")

    def test_vector_selection(self):
        self.check("""implicit none
complex::z(2,2)
integer::idx(2)
z=reshape([(1.,2.),(3.,4.),(5.,6.),(7.,8.)],[2,2])
idx=[2,1]
z(idx,idx)=z
print *,z
print *,z%re
end
""", "(7,8) (5,6) (3,4) (1,2)\n7 5 3 1\n")

    def test_literal_without_complex_declaration(self):
        self.check("""implicit none
real,parameter::x=1.,y=2.
print *, (x,y),real(cmplx(x=x,y=y,kind=8)),kind(cmplx(x,kind=8))
end
""", "(1,2) 1 8\n")

    def test_large_magnitude_sqrt(self):
        # Python cmath supplies this overflow-boundary oracle. The installed
        # Windows gfortran runtime itself returns infinities for this input.
        self.check("""implicit none
complex(8)::z,w
z=(1.7d308,1.7d308)
w=sqrt(z)
print *,abs(real(w)/1.d154-1.4325088230154572)<1.d-12
print *,abs(aimag(w)/1.d154-.593364582712122)<1.d-12
end
""", "T\nT\n", reference=False)

    def test_generic_argument_types(self):
        self.check("""module kinds
implicit none
interface which
module procedure c4,c8,r8
end interface
contains
integer function c4(z)
complex,intent(in)::z
c4=4
end function
integer function c8(z)
complex(8),intent(in)::z
c8=8
end function
integer function r8(z)
real(8),intent(in)::z
r8=80
end function
end module
program main
use kinds
implicit none
complex(8)::z
z=(1.d0,2.d0)
print *,which(cmplx(1,2)),which((1.d0,2.d0)),which(real(z)),which(abs(z)),which(z%im)
end program
""", "4 8 80 80 80\n")

    def test_computed_complex_output(self):
        self.check("""implicit none
complex::z,w,a(2)
z=(3.,4.)
w=(1.,-2.)
a=[(1.,2.),(3.,4.)]
print *,sum(a),dot_product(a,a),sqrt(z),z*w
end
""", "(4,6) (30,0) (2,1) (11,-2)\n")

    def test_demo(self):
        with tempfile.TemporaryDirectory(prefix="complex-demo-") as tmp:
            self.compare(ROOT/"tests/programs/xcomplex.f90", """z = (3,4)
conjg(z) = (3,-4)
abs(z) = 5
values = (3,4) (3,-4) (25,0)
dot_product(values,values) = (675,0)
""", Path(tmp))

    def test_list_directed_spacing(self):
        with tempfile.TemporaryDirectory(prefix="complex-spacing-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text("""implicit none
complex::z(2)
z=[(1.,2.),(3.,4.)]
print *, 'A',z,'B',z(1),7,'C'
print *,z(1),'D'//'E'
print *,'A  ','B '
end
""",encoding="utf-8")
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,encoding="utf-8",timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    # Component number formatting may differ between interpreter
                    # and native output. All surrounding whitespace is exact.
                    text=re.sub(r"\([^)]*\)","Z",result.stdout)
                    self.assertEqual(text," A Z Z B Z 7 C\n Z DE\n A  B \n")

    def test_invalid_complex(self):
        for body, diagnostic in [
            ("complex(16)::z\nz=(1.,2.)", "unsupported COMPLEX kind 16"),
            ("complex::z\nz=(1.,2.)\nprint *,z<(3.,4.)", "ordered comparisons are not defined"),
            ("complex::z\nprint *,real(z)", "uninitialized variable"),
        ]:
            with self.subTest(body=body), tempfile.TemporaryDirectory(prefix="complex-invalid-") as tmp:
                path=Path(tmp)/"invalid.f90"
                path.write_text("implicit none\n"+body+"\nend\n",encoding="utf-8")
                result=subprocess.run([COMPILER,"emit-obj",path,Path(tmp)/"invalid.obj"],capture_output=True,text=True,encoding="utf-8",timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(diagnostic,result.stderr)
                self.assertNotIn("panicked",result.stderr)

if __name__ == "__main__":
    unittest.main()
