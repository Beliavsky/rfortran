"""Kind resolution and keyword calls through an existing compiler frontend."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from compiler_frontend import FrontendError, Unsupported, find_compiler
from transpile import transpile

try:
    find_compiler()
    HAS_FRONTEND = True
except FrontendError:
    HAS_FRONTEND = False


@unittest.skipUnless(HAS_FRONTEND, "Existing rfort executable required")
class NumericKindsTests(unittest.TestCase):
    def run_source(self, source):
        code = transpile(source)
        if not shutil.which("rustc"):
            self.skipTest("rustc required")
        with tempfile.TemporaryDirectory(prefix="rust-kinds-test-", dir=ROOT.parent / "target") as directory:
            work = Path(directory)
            rust = work / "main.rs"
            exe = work / ("main.exe" if sys.platform == "win32" else "main.bin")
            rust.write_text(code, encoding="utf-8")
            compiled = subprocess.run(["rustc", "--edition=2021", "-D", "warnings", str(rust), "-o", str(exe)],
                                      capture_output=True, text=True, timeout=120)
            self.assertEqual(compiled.returncode, 0, compiled.stderr + code)
            result = subprocess.run([str(exe)], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            return result.stdout.split(), code

    def test_kind_module_example(self):
        source = (ROOT.parent / "kind.f90").read_text() + "\n" + (ROOT.parent / "xkind.f90").read_text()
        output, _ = self.run_source(source)
        self.assertEqual(list(map(float, output)), [8,4,8,4,8,-999,-999])

    def test_literal_kinds_and_selected_kind_inquiries(self):
        output, code = self.run_source("""implicit none
integer,parameter :: sp=selected_real_kind(p=6,r=37), dp=selected_real_kind(r=307,p=15)
integer,parameter :: ik=selected_int_kind(r=9), alias=dp
real(kind=sp) :: x
real(alias) :: y
integer(kind=ik) :: i
x=1.25_sp
y=1.23456789012345_alias + 2.5e-2_8
i=3_ik
print *,kind(x),kind(y),kind(2.0_4),kind(2.0_8),kind(i),i
print *,x,y
print *,selected_real_kind(32),selected_real_kind(r=308),selected_real_kind(32,400)
print *,selected_real_kind(6,37,10),selected_int_kind(19)
end
""")
        self.assertEqual(list(map(int, output[:6])), [4,8,4,8,4,3])
        self.assertAlmostEqual(float(output[6]), 1.25)
        self.assertAlmostEqual(float(output[7]), 1.25956789012345, places=14)
        self.assertEqual(list(map(int,output[8:])), [-1,-2,-3,-5,-1])
        self.assertIn("1.23456789012345e0f64", code)

    def test_iso_fortran_env_renames_and_reexports(self):
        output, code = self.run_source("""module kinds
use, intrinsic :: iso_fortran_env, only: dp=>real64, sp=>real32
implicit none
private
public :: dp, sp
end module
program p
use kinds, only: working=>dp, sp
implicit none
real(working) :: x
real(sp) :: y
x=2.125_working
y=1.5_sp
print *,kind(x),kind(y),x+y
end program
""")
        self.assertEqual(list(map(float,output)), [8,4,3.625])
        self.assertIn("x: f64",code)
        self.assertIn("y: f32",code)

    def test_module_public_attribute_and_host_association(self):
        output, _ = self.run_source("""module m
implicit none
private
integer,parameter,public :: dp=kind(1.0d0)
real(dp),parameter,public :: missing=-999.0_dp
public :: twice
contains
function twice(x) result(y)
real(dp),intent(in) :: x
real(dp) :: y
y=x*2.0_dp
end function
end module
program p
use m
implicit none
real(dp) :: x
x=1.25_dp
print *,twice(x),missing
end program
""")
        self.assertEqual(list(map(float,output)), [2.5,-999])

    def test_local_kind_parameter_shadows_module_parameter(self):
        output, _ = self.run_source("""module m
implicit none
integer,parameter :: dp=8
contains
function f(x) result(y)
integer,parameter :: dp=4
real(dp),intent(in) :: x
real(dp) :: y
y=x+0.5_dp
end function
end module
program p
use m
implicit none
real(dp) :: x
x=2.0_dp
print *,f(1.0),kind(x)
end program
""")
        self.assertEqual(list(map(float,output)), [1.5,8])

    def test_unsupported_or_unresolved_kinds_are_diagnosed(self):
        for declaration, message in (
            ("real(16) :: x", "unsupported REAL kind 16"),
            ("integer(8) :: x", "unsupported INTEGER kind 8"),
            ("integer(0) :: x", "unsupported INTEGER kind 0"),
            ("real(missing) :: x", "previously defined integer PARAMETER"),
            ("real :: x\nx=1.0_16", "unsupported REAL kind 16"),
            ("integer :: x\nx=1_8", "unsupported INTEGER kind 8"),
        ):
            with self.subTest(declaration=declaration), self.assertRaisesRegex(Unsupported, message):
                transpile("implicit none\n"+declaration+"\nend\n")

    def test_private_kind_constant_remains_inaccessible(self):
        with self.assertRaisesRegex(Unsupported,"previously defined integer PARAMETER"):
            transpile("""module m
implicit none
private
integer,parameter :: dp=8
end module
program p
use m
implicit none
real(dp) :: x
end program
""")

    def test_keyword_subroutine_example(self):
        output, _ = self.run_source((ROOT.parent / "xnamed_arg.f90").read_text())
        self.assertEqual(list(map(float,output)), [10,20,30])

    KEYWORD_MODULE = """module m
implicit none
contains
function difference(left,right) result(value)
integer,intent(in) :: left,right
integer :: value
value=left-right
end function
subroutine change(input,output,total)
integer,intent(in) :: input
integer,intent(out) :: output
integer,intent(inout) :: total
output=input*2
total=total+input
end subroutine
end module
"""

    def test_reordered_function_and_subroutine_keywords(self):
        output, _ = self.run_source(self.KEYWORD_MODULE+"""program p
use m
implicit none
integer :: x,y
x=10
call change(total=x,output=y,input=difference(right=3,left=8))
print *,x,y,difference(9,right=2)
end program
""")
        self.assertEqual(list(map(int,output)), [15,10,7])

    def test_invalid_keyword_association_is_diagnosed(self):
        for call, message in (
            ("print *,difference(wrong=1,right=2)", "unknown keyword"),
            ("print *,difference(1,left=2)", "duplicate argument"),
            ("print *,difference(left=1,2)", "positional argument after keyword"),
            ("print *,difference(left=1)", "wrong number of arguments"),
            ("call change(input=1,output=2,total=x)", "output arguments"),
        ):
            with self.subTest(call=call), self.assertRaisesRegex(Unsupported,message):
                transpile(self.KEYWORD_MODULE+"program p\nuse m\nimplicit none\ninteger :: x\nx=0\n"+call+"\nend program\n")


if __name__ == "__main__":
    unittest.main()
