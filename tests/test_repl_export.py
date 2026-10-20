"""REPL source export; validate generated programs independently with gfortran.

RFORT_TEST_COMPILER selects the isolated REPL development build.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER


class ReplExport(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(prefix="rfort-export-")
        self.addCleanup(self.work.cleanup)
        self.directory = Path(self.work.name)
        self.source = self.directory / "saved session.f90"

    def session(self, source):
        result = subprocess.run([COMPILER, "repl"], input=source, text=True,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def exported_run(self):
        compiler = shutil.which("gfortran")
        if compiler is None:
            return None
        executable = self.directory / ("saved.exe" if os.name == "nt" else "saved")
        result = subprocess.run([compiler, "-std=f2018", "-pedantic-errors",
                                 str(self.source), "-o", str(executable)],
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr + self.source.read_text())
        result = subprocess.run([executable], text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.split()

    def test_explicit_declarations_initializers_blocks_and_expressions(self):
        result = self.session(f"""integer, parameter :: k=2
integer :: n=1, a(2)=[4,5], i
n=n+1; print '(i3)',n
integer :: m=7
character(5) :: s='a=;!'
do i=1,k
n=n+a(i)
end do
n+m
print '(a)',s
:save "{self.source}"
:q
""")
        self.assertEqual(result.stderr, "")
        self.assertEqual(result.stdout, "  2\n 18\na=;! \n")
        source = self.source.read_text()
        self.assertEqual(source.count("implicit none"), 1)
        self.assertIn("integer, parameter :: k=2", source)
        self.assertIn("n = 1", source)
        self.assertIn("a = [4,5]", source)
        self.assertIn("m = 7", source)
        self.assertIn("print *, n+m", source)
        self.assertLess(source.index("integer :: m"), source.index("n = 1"))
        self.assertLess(source.index("n=n+1"), source.index("m = 7"))
        self.assertNotIn("@", source)
        self.assertEqual(self.exported_run(), ["2", "18", "a=;!"] if shutil.which("gfortran") else None)

    def test_auto_declarations_allocations_and_self_references(self):
        result = self.session(f""":auto on
x=[10.,20.,30.]
x=[x,40.]
n=size(x)
print *,n,int(sum(x))
d=[1d0,2d0]
mask=d>1d0
s='hello'
print *,count(mask)
:vars
:save {self.source}
:q
""")
        self.assertEqual(result.stderr, "")
        source = self.source.read_text()
        self.assertIn("real, allocatable :: x(:)", source)
        self.assertIn("double precision, allocatable :: d(:)", source)
        self.assertIn("logical, allocatable :: mask(:)", source)
        self.assertIn("character(len=5) :: s", source)
        self.assertIn("x=[x,40.]", source)
        self.assertNotIn(":auto", source)
        self.assertNotIn(":vars", source)
        self.assertNotIn("snapshot", source)
        self.assertEqual(self.exported_run(), ["4", "100", "1"] if shutil.which("gfortran") else None)

    def test_failed_submissions_are_not_recorded(self):
        result = self.session(f"""integer :: n=5,z=0
n=8; print *,99; n=n/z
unknown+1
integer :: rejected=n*2
n
:save "{self.source}"
:q
""")
        self.assertIn("integer division by zero", result.stderr)
        self.assertIn("undefined variable", result.stderr)
        self.assertIn("initializer", result.stderr)
        self.assertEqual(result.stdout, " 99\n 5\n")
        source = self.source.read_text()
        for excluded in ("99", "n=8", "n/z", "unknown", "rejected"):
            self.assertNotIn(excluded, source)
        self.assertEqual(self.exported_run(), ["5"] if shutil.which("gfortran") else None)

    def test_reset_cancel_and_overwriting_a_saved_file(self):
        result = self.session(f"""integer :: discarded=7
:save "{self.source}"
:reset
integer :: n=3
if(.true.) then
n=9
:save "{self.source}"
:cancel
n
:save "{self.source}"
:q
""")
        self.assertEqual(result.stdout, " 3\n")
        self.assertIn("finish or :cancel", result.stderr)
        source = self.source.read_text()
        self.assertNotIn("discarded", source)
        self.assertNotIn("n=9", source)
        self.assertNotIn("if(", source)
        self.assertEqual(self.exported_run(), ["3"] if shutil.which("gfortran") else None)

    def test_rng_calls_continuations_and_source_strings_survive(self):
        result = self.session(f"""real :: x,y
call random_number(x)
call random_number(y)
integer :: n=1, & ! declaration continuation
 & m=2
print *, & ! executable continuation
 & n+m
print *, 'a;!b'
:save "{self.source}"
:q
""")
        self.assertEqual(result.stderr, "")
        source = self.source.read_text()
        self.assertEqual(source.count("call random_number"), 2)
        self.assertNotIn("0.280835", source)
        self.assertIn("'a;!b'", source)
        self.assertEqual(self.exported_run(), ["3", "a;!b"] if shutil.which("gfortran") else None)

    def test_empty_session_and_save_errors(self):
        result = self.session(f""":save
:save "unclosed
:save "{self.directory}"
:save "{self.source}"
:q
""")
        self.assertIn("usage: :save", result.stderr)
        self.assertIn("missing closing quote", result.stderr)
        self.assertIn("could not save", result.stderr)
        self.assertEqual(self.exported_run(), [] if shutil.which("gfortran") else None)

    def test_program_name_does_not_conflict_with_a_variable(self):
        result = self.session(f"""integer :: repl_session=3
repl_session
:save "{self.source}"
:q
""")
        self.assertEqual(result.stderr, "")
        self.assertIn("program repl_session_export", self.source.read_text())
        self.assertEqual(self.exported_run(), ["3"] if shutil.which("gfortran") else None)


if __name__ == "__main__":
    unittest.main()
