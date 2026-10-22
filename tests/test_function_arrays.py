"""Fixed array function arguments in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent
import test_deallocate

PROGRAM_SECTIONS = """module m
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
pure function mean(x) result(y)
real(dp), intent(in) :: x(:)
real(dp) :: y
y = sum(x)/max(1,size(x))
end function
pure function mean_by_row(xx) result(xmean)
real(dp), intent(in) :: xx(:,:)
real(dp) :: xmean(size(xx,1))
integer :: i
do i=1,size(xx,1)
   xmean(i) = mean(xx(i,:))
end do
end function
pure function mean_by_col(xx) result(xmean)
real(dp), intent(in) :: xx(:,:)
real(dp) :: xmean(size(xx,2))
integer :: i
do i=1,size(xx,2)
   xmean(i) = mean(xx(:,i))
end do
end function
function tensor(xx) result(y)
real(dp), intent(in) :: xx(:,:,:)
real(dp) :: y(size(xx,2),size(xx,3))
integer :: i,j
do j=1,size(xx,3)
do i=1,size(xx,2)
   y(i,j) = mean(xx(:,i,j))
end do
end do
end function
end module
use m
real(dp) :: a(2,3), t(2,2,2)
real(dp), allocatable :: b(:,:)
integer :: k
a = reshape([(real(k,dp),k=1,6)],[2,3])
t = reshape([(real(k,dp),k=1,8)],[2,2,2])
allocate(b(3,2))
b = reshape([(real(10*k,dp),k=1,6)],[3,2])
print '(3f8.2)', mean_by_row(a)
print '(3f8.2)', mean_by_col(a)
print '(3f8.2)', mean_by_row(b)
print '(3f8.2)', mean_by_col(b)
print '(4f8.2)', tensor(t)
end
"""
EXPECTED_SECTIONS = '    3.00    4.00\n    1.50    3.50    5.50\n   25.00   35.00   45.00\n   20.00   50.00\n    1.50    3.50    5.50    7.50\n'

class FunctionArrays(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_constructor_bounds_from_size_of_dummies(self):
        """Implied-DO bounds SIZE(xx) of an assumed-shape dummy in constructor
        arguments, as in statistics.f90 r2_trend."""
        self.check("module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ncontains\npure function cov(x,y) result(c)\nreal(dp), intent(in) :: x(:), y(:)\nreal(dp) :: c\nc = sum((x-sum(x)/size(x))*(y-sum(y)/size(y)))/size(x)\nend function\nfunction trend(xx) result(yy)\nreal(dp), intent(in) :: xx(:)\nreal(dp) :: yy\ninteger :: i\nyy = cov(xx,1.0_dp*(/(i,i=1,size(xx))/)) + cov(xx,[(real(2*i,dp),i=size(xx),1,-1)])\nend function\nend module\nuse m\nreal(dp), allocatable :: v(:)\nv = [1.0_dp, 4.0_dp, 2.0_dp, 8.0_dp]\nprint '(f10.4)', trend([1.0_dp, 2.0_dp, 4.0_dp])\nprint '(f10.4)', trend(v)\nend\n", "   -1.0000\n   -2.3750\n")

    def test_selections_from_assumed_shape_dummies(self):
        """A vector subscript by a section of an automatic index array, and a
        strided section, of an assumed-shape dummy as function arguments, as
        in statistics.f90 expectile."""
        self.check("module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ncontains\npure function mean(x) result(y)\nreal(dp), intent(in) :: x(:)\nreal(dp) :: y\ny = sum(x)/max(1,size(x))\nend function\nfunction expectile(perc,xx) result(yy)\nreal(dp), intent(in) :: perc, xx(:)\nreal(dp) :: yy\ninteger :: i,n,k,irank(size(xx))\nn = size(xx)\ni = min(n,max(1,nint(perc*n)))\ndo k=1,n\n  irank(k) = n+1-k\nend do\nyy = mean(xx(irank(:i))) + mean(xx(2:n:2))\nend function\nend module\nuse m\nreal(dp), allocatable :: v(:)\nv = [1.0_dp, 4.0_dp, 2.0_dp, 8.0_dp]\nprint '(f10.4)', expectile(0.5_dp, v)\nprint '(f10.4)', expectile(0.25_dp, [3.0_dp, 5.0_dp, 7.0_dp])\nend\n", '   11.0000\n   12.0000\n')

    def test_sections_of_local_allocatables_and_runtime_merge(self):
        """Sections of a rank-two ALLOCATABLE and of a dummy in one call, and
        MERGE with a runtime-sized mask, inside ASSOCIATE, as in statistics.f90
        print_stats_ranges_madev."""
        self.check('module m\nimplicit none\ncontains\npure function dot(x,y) result(c)\nreal, intent(in) :: x(:), y(:)\nreal :: c\nc = sum(x*y)\nend function\nsubroutine s(d, n)\nreal, intent(in) :: d(:,:)\ninteger, intent(in) :: n\nreal, allocatable :: b(:,:)\ninteger :: j\nallocate(b(4,2))\nb = 1.0\nj = 2\nprint *, dot(b(2:n, j), d(2:, 1))\nprint *, dot(d(2:, 1), b(2:n, j))\nend subroutine\nend module\nuse m\nreal, allocatable :: a(:,:)\nallocate(a(3,2))\na = 2.0\ncall s(a, 3)\nend\n','   4.00000000    \n   4.00000000    \n')
        self.check('module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ncontains\npure function correl(x,y) result(c)\nreal(dp), intent(in) :: x(:), y(:)\nreal(dp) :: c\nc = sum(x*y)\nend function\nsubroutine p(xsignal, xret, nma)\nreal(dp), intent(in) :: xsignal(:,:), xret(:,:)\ninteger, intent(in) :: nma\nreal(dp), allocatable :: xmadev(:,:)\ninteger :: j, k, nsignal\nnsignal = size(xsignal,1)\nxmadev = 2.0_dp * xsignal\ndo j=1,size(xsignal,2)\n   do k=1,size(xret,2)\n   associate (xindep => xmadev(nma:nsignal-1, j), xdep => xret(nma:, k))\n   print "(i0,1x,a,*(1x,f8.4))", nma, "x", correl(xindep, xdep), correl(merge(0.0_dp, 1.0_dp, xindep <= 2.0_dp), xdep)\n   end associate\n   end do\nend do\nend subroutine\nend module\nuse m\nreal(dp), allocatable :: a(:,:), r(:,:)\ninteger :: i\nallocate(a(4,2), r(3,2))\na = reshape([(real(i,dp),i=1,8)],[4,2])\nr = reshape([(real(10*i,dp),i=1,6)],[3,2])\ncall p(a, r, 2)\nend\n','2 x 260.0000  50.0000\n2 x 560.0000 110.0000\n2 x 660.0000  50.0000\n2 x ******** 110.0000\n')

    def test_sections_of_assumed_shape_dummies(self):
        """Rows, columns and rank-three sections of assumed-shape dummies as
        function arguments, from fixed and allocatable actual arrays, as in
        statistics.f90 mean_by_row and stat_tensor_str."""
        self.check(PROGRAM_SECTIONS, EXPECTED_SECTIONS)
    def test_execution(self):
        source=ROOT / "tests/92_function_arrays.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="function-arrays-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout,expected)
                    self.assertEqual(result.stderr,"")
            if shutil.which("gfortran"):
                exe=directory/"reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],cwd=directory,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(expected,result.stdout),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("integer::a(2)\na=1", "a", "integer,intent(in)::x(3)", True, "matching extent"),
            ("real::a(3)\na=1.0", "a", "integer,intent(in)::x(3)", True, "type or rank mismatch"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-function-arrays-") as tmp:
            source=Path(tmp)/"main.f90"
            for decl,actual,dummy,pure,message in cases:
                source.write_text("program main\n"+decl+"\nprint *,f("+actual+")\ncontains\n"+("pure " if pure else "")+"integer function f(x) result(n)\n"+dummy+"\nn=sum(x)\nend function f\nend program main\n")
                with self.subTest(dummy=dummy,actual=actual):
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                    self.assertIn(message,result.stderr)

    def test_input_initialization(self):
        # Descriptor association preserves initialization checks on elements
        # actually read by the function, rather than reading all inputs on entry.
        with tempfile.TemporaryDirectory(prefix="function-array-unset-") as tmp:
            source=Path(tmp)/"main.f90"
            source.write_text("integer::a(3)\na(1)=1\nprint *,f(a)\ncontains\npure integer function f(x) result(n)\ninteger,intent(in)::x(3)\nn=sum(x)\nend function f\nend\n")
            for command in build_commands(source,Path(tmp)):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                self.assertIn("uninitialized",result.stderr)

    def test_ordinary_functions_sections_and_large_fixed_arrays(self):
        import test_deallocate
        test_deallocate.Deallocate.check(self,"""program main
integer::a(1025),b(3)
a=1;b=2
print *,sum_large(a),sum_three(b(:))
contains
pure integer function sum_large(x) result(n)
integer,intent(in)::x(1025)
n=sum(x)
end function sum_large
integer function sum_three(x) result(n)
integer,intent(in)::x(3)
n=sum(x)
end function sum_three
end program main
""",'1025 6\n')

if __name__=="__main__":
    unittest.main()
