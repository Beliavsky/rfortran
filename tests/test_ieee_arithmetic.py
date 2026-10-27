"""IEEE module imports and elemental operations, compared with gfortran."""
import unittest
import os
import subprocess
import tempfile
from pathlib import Path
import test_deallocate as helpers


class IeeeArithmetic(unittest.TestCase):
    def setUp(self):
        # Some cases intentionally define a user IEEE_ARITHMETIC module.
        # Keep their .rmod files out of other cases and the repository root.
        directory = tempfile.TemporaryDirectory(prefix="ieee-arithmetic-work-")
        self.addCleanup(directory.cleanup)
        previous = Path.cwd()
        self.addCleanup(os.chdir, previous)
        os.chdir(directory.name)

    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_example(self):
        source = helpers.ROOT / 'tests/programs/xieee_arithmetic.f90'
        self.check(source.read_text(encoding='utf-8'), 'F T F\nT F F\nT F F\nF T F\nF F F\n')

    def test_scalar_classes_both_kinds(self):
        for kind in (4, 8):
            with self.subTest(kind=kind):
                self.check(f'''use, intrinsic :: ieee_arithmetic
implicit none
real(kind={kind}) :: x, y
x=ieee_value(0.0_{kind},ieee_quiet_nan)
y=ieee_value(0.0_{kind},ieee_positive_inf)
print *,ieee_is_nan(x),ieee_is_nan(y),ieee_is_finite(x),ieee_is_finite(y)
print *,ieee_unordered(x,y),ieee_unordered(y,1.0_{kind})
print *,y>huge(y),ieee_value(y,ieee_negative_inf)<-huge(y)
x=ieee_value(y,ieee_positive_denormal)
print *,x>0,x<tiny(x),ieee_is_normal(x),ieee_is_finite(x)
x=ieee_value(y,ieee_negative_normal)
print *,x<0,ieee_is_normal(x)
x=ieee_value(y,ieee_negative_zero)
print *,x==0,ieee_is_normal(x)
print *,1.0_{kind}/x == ieee_value(0.0_{kind},ieee_negative_inf)
end
''', 'T F F F\nT F\nT T\nT T F T\nT T\nT T\nT\n')

    def test_arrays_keywords_renaming_and_purity(self):
        self.check('''module m
use ieee_arithmetic, only: nan => ieee_is_nan, value => ieee_value, ieee_quiet_nan
implicit none
contains
pure function missing(x) result(mask)
real(kind=8),intent(in)::x(:,:)
logical::mask(size(x,1),size(x,2))
mask=nan(x=x)
end function
end module
program main
use m
implicit none
real(kind=8),allocatable::a(:,:)
real(kind=8)::b(2,2)
allocate(a(2,2))
a=reshape([1.0_8,value(class=ieee_quiet_nan,x=0.0_8),3.0_8,4.0_8],[2,2])
print *,missing(a)
b=a
where(nan(b)) b=0
print *,sum(b)
end program
''', 'F T F F\n8\n')

    def test_elemental_value_and_class_arrays(self):
        self.check("""use ieee_arithmetic
implicit none
real :: a(3)
real(kind=8) :: b(2)
a=ieee_value([0.0,1.0,2.0],ieee_quiet_nan)
print *,ieee_is_nan(a)
b=ieee_value(0.0_8,[ieee_quiet_nan,ieee_positive_inf])
print *,ieee_is_nan(b),ieee_is_finite(b)
end
""", 'T T T\nT F F F\n')

    def test_module_constant_generic(self):
        self.check("""module m
implicit none
private
public :: token, t, pick
type t
integer :: n
end type
type(t),parameter :: token=t(17)
interface pick
module procedure pick_t
end interface
contains
integer function pick_t(x) result(n)
type(t),intent(in)::x
n=x%n
end function
end module
use m, only: token, pick
implicit none
print *,pick(token)
end
""", '17\n')

    def test_separate_compilation(self):
        with tempfile.TemporaryDirectory(prefix="ieee-module-") as tmp:
            directory = Path(tmp)
            (directory / "m.f90").write_text("""module m
use ieee_arithmetic, only: ieee_value, ieee_quiet_nan, nan => ieee_is_nan
contains
logical function check_nan() result(ok)
ok=nan(ieee_value(0.0,ieee_quiet_nan))
end function
end module
""", encoding="utf-8")
            (directory / "main.f90").write_text("use m\nimplicit none\nprint *,check_nan()\nend\n", encoding="utf-8")
            for command in ([helpers.COMPILER, '-c', 'm.f90'],
                            [helpers.COMPILER, 'main.f90', 'm.obj' if os.name == 'nt' else 'm.o', '-o', 'main.exe']):
                result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([directory / 'main.exe'], cwd=directory, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), 'T')

    def test_intrinsic_and_user_module_namespaces(self):
        self.check("""module ieee_arithmetic
integer,parameter :: token=42
end module
program main
use, non_intrinsic :: ieee_arithmetic, only: token
use, intrinsic :: ieee_arithmetic, only: ieee_is_nan
implicit none
print *,token,ieee_is_nan(x=1.0)
end program
""", '42 F\n')

    def test_type_checks(self):
        self.reject('''use ieee_arithmetic
implicit none
print *,ieee_value(0.0,2)
end
''', 'no matching specific procedure')


if __name__ == '__main__':
    unittest.main()
