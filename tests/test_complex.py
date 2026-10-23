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

    def test_components_of_derived_types(self):
        """COMPLEX scalar and array components, in a nested type, through
        component designators, %RE and %IM, intrinsics and arguments."""
        self.check('module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ntype :: wave\n   real :: amp\n   complex :: z\n   complex(kind=dp) :: zd(2)\nend type\ntype :: outer\n   integer :: id\n   type(wave) :: w\nend type\ncontains\nfunction norm2c(z) result(r)\ncomplex, intent(in) :: z\nreal :: r\nr = abs(z)\nend function\nend module\nprogram p\nuse m\nimplicit none\ntype(wave) :: a\ntype(outer) :: o\na%amp = 2.0\na%z = (3.0, 4.0)\na%zd(1) = cmplx(1.0_dp, -1.0_dp, kind=dp)\na%zd(2) = a%zd(1) * (0.0_dp, 1.0_dp)\nprint *, a%amp, a%z\nprint *, a%z%re, a%z%im, abs(a%z), norm2c(a%z), real(a%z), aimag(a%z)\nprint *, a%zd\na%z = a%z * 2 + conjg(a%z)\nprint *, a%z\no%id = 7\no%w%z = (1.5, -2.5)\no%w%zd = (0.5_dp, 0.25_dp)\nprint *, o%id, o%w%z, o%w%zd(2), o%w%z%im\nend program\n', '   2.00000000                 (3.00000000,4.00000000)\n   3.00000000       4.00000000       5.00000000       5.00000000       3.00000000       4.00000000    \n              (1.0000000000000000,-1.0000000000000000)               (1.0000000000000000,1.0000000000000000)\n             (9.00000000,4.00000000)\n           7            (1.50000000,-2.50000000)             (0.50000000000000000,0.25000000000000000)  -2.50000000    \n')

    def test_components_of_record_arrays_and_whole_objects(self):
        """COMPLEX components of elements of an array of records, an
        INTENT(OUT) component argument, and list-directed output of whole
        objects with COMPLEX components."""
        self.check('module m\nimplicit none\ntype :: wave\n   real :: amp\n   complex :: z\nend type\ncontains\nsubroutine setz(z)\ncomplex, intent(out) :: z\nz = (0.5, 1.5)\nend subroutine\nend module\nprogram p\nuse m\nimplicit none\ntype(wave) :: a, arr(3)\ninteger :: i\na%amp = 1.0\ncall setz(a%z)\nprint *, a\ndo i = 1, 3\n   arr(i)%amp = i\n   arr(i)%z = cmplx(i, -i)\nend do\nprint *, arr(2)%z, arr(3)%z%im\nprint *, arr\nend program\n', '   1.00000000                (0.500000000,1.50000000)\n            (2.00000000,-2.00000000)  -3.00000000    \n   1.00000000                (1.00000000,-1.00000000)   2.00000000                (2.00000000,-2.00000000)   3.00000000                (3.00000000,-3.00000000)\n')

    def test_associate_with_complex_expression(self):
        """ofort's xassoc_complex.f90: %IM of an ASSOCIATE name whose
        selector is a COMPLEX expression, with KIND and STORAGE_SIZE."""
        self.check('program hello\n  use, intrinsic :: iso_fortran_env, only: wp => real64\n  implicit none\n\n  real(wp) :: x = 0.0_wp\n  real(wp) :: h = 1.0e-50_wp\n\n\n  associate (r => f(cmplx(x, h, kind=wp)))\n    print *, "kind(r):", kind(r)\n    print *, "storage_size(r):", storage_size(r)\n    print *, "r = ", r\n    print *, 1.0_wp / h * r%im  ! the correct result should be 1.0\n    print *, 1.0_wp / h * aimag(r)  ! the correct result should be 1.0\n  end associate\n\ncontains\n\n  complex(wp) function f(x)\n    complex(wp), intent(in) :: x\n    f = sin(x)\n  end function f\n\nend program hello\n', ' kind(r):           8\n storage_size(r):         128\n r =          (0.0000000000000000,1.00000000000000001E-050)\n   1.0000000000000000     \n   1.0000000000000000     \n')

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
