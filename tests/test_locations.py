"""Location intrinsics in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent
import test_deallocate

class LocationTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/108_locations.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="reshape-constructors-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertTrue(equivalent(result.stdout,expected),result.stdout)
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
            ("print *,minloc(1)", "ARRAY must be an array"),
            ("print *,maxloc([1,2],dim=0)", "DIM must be between"),
            ("integer::d\nd=1\nprint *,minloc([1,2],dim=d)", "constant INTEGER"),
            ("print *,minloc([1,2],mask=[.true.])", "MASK shape"),
            ("print *,minloc([1,2],mask=1)", "MASK must be LOGICAL"),
            ("print *,maxloc([1,2],back=1)", "BACK must be scalar LOGICAL"),
            ("print *,minloc([1,2],back=[.true.])", "BACK must be scalar LOGICAL"),
            ("print *,minloc([.true.,.false.])", "must be numeric"),
            ("print *,findloc([1,2])", "requires VALUE"),
            ("print *,findloc([1,2],[1])", "VALUE must be scalar"),
            ("print *,findloc([1,2],1.0)", "same type and kind"),
            ("print *,maxloc([1,2],kind=2)", "KIND=4 or KIND=8"),
            ("print *,minloc([1,2],dim=1,dim=1)", "duplicate"),
        ]
        with tempfile.TemporaryDirectory(prefix="locations-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_kinds_and_masked_reads(self):
        program="""program main
integer::a(4),empty(0)
integer(8)::wide(3),needle
logical::mask(4)
double precision::d(3),target
mask=[.false.,.true.,.false.,.true.]
a(2)=8
a(4)=3
print *,minloc(a,mask=mask),maxloc(a,mask=mask),findloc(a,3,mask=mask)
wide=huge(wide)
needle=huge(wide)
print *,minloc(wide),maxloc(wide,back=.true.),findloc(wide,needle,back=.true.)
d=[2.0d0,-1.0d0,-1.0d0]
target=-1.0d0
print *,minloc(d,back=.true.),findloc(d,target,back=.true.)
print *,10,empty,20
print *,empty
end program main
"""
        with tempfile.TemporaryDirectory(prefix="locations-kinds-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"4 2 4\n1 3 3\n3 3\n10 20\n\n"),result.stdout)

    check = test_deallocate.Deallocate.check

    def test_runtime_arrays(self):
        """MAXLOC, MINLOC, FINDLOC and SIZE of ALLOCATABLE arrays and their expressions."""
        self.check("""program p
integer, allocatable :: a(:)
real, allocatable :: x(:)
integer :: n, i, k(1)
n = 6
allocate(x(n))
a = [2, 7, 1, 7, -3, 0]
do i = 1, n
  x(i) = sin(real(i))
end do
print '(*(i0,1x))', maxloc(a), minloc(a), maxloc(a, back=.true.), findloc(a, 7), findloc(a, 7, back=.true.)
print '(*(i0,1x))', maxloc(a, dim=1), minloc(a, mask=a > 0), findloc(a, 99), maxloc(-a)
print '(*(i0,1x))', maxloc(x), minloc(x), maxloc(x, mask=x < 0.5), minloc(abs(x))
k = maxloc(a(2:5))
print '(*(i0,1x))', k, size(a)
deallocate(a)
allocate(a(0))
print '(*(i0,1x))', maxloc(a), findloc(a, 1)
end program
""", '2 5 4 2 4\n2 3 0 5\n2 5 3 3\n1 6\n0 0\n')
        self.check("""program p
integer, allocatable :: a(:)
real, allocatable :: x(:)
character(len=:), allocatable :: s(:)
integer :: b(4)
a = [3, 1, 2]
allocate(x(5))
x = 1.5
b = 0
s = [character(len=6) :: 'apple', 'kiwi']
print '(*(i0,1x))', size(a+1), size(-a), size(abs(a)*2), size(sqrt(x)), size(x, 1), size(len_trim(s)), size(index(s,'i')), size(b+1)
print '(*(i0,1x))', maxloc(len_trim(s)), minloc(abs(a-2))
end program
""", '3 3 3 5 5 2 2 4\n1 3\n')

if __name__=="__main__":
    unittest.main()
