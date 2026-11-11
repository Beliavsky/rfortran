"""Run after cargo build --bin rfort: python tests/test_output_implied_do.py."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, COMPILER, build_commands


class OutputImpliedDo(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="output-implied-do-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def source(self, text):
        source = self.directory / "example.f90"
        source.write_text(text)
        return source

    def test_output_and_reference(self):
        source = ROOT / "tests/75_output_implied_do.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        for cmd in build_commands(source, self.directory):
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, "")
            self.assertEqual(result.stdout, expected)
        if shutil.which("gfortran"):
            exe = self.directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([s.split() for s in expected.splitlines()], [s.split() for s in result.stdout.splitlines()])

    def test_sections_beside_implied_do(self):
        # Reductions of sections, rank-one sections of a matrix, and elemental
        # expressions of them, in the same list as an implied-DO.
        source = self.source("""implicit none
integer :: i, j
real :: a(2,3), v(4)
a = reshape([1.,2.,3.,4.,5.,6.], [2,3])
v = [10., 20., 30., 40.]
j = 2
print *, (a(i,j), i=1,2), sum(a(:,j))
write (*,*) (i, i=1,2), maxval(a(1,:)), dot_product(a(:,1), a(:,3))
print *, (i, i=1,1), a(:,j), a(2,:)
print *, (i, i=1,1), 2*a(:,j) + 1, sqrt(a(1,2:3)), v(2:4:2)
print *, (i, i=1,1), size(a), product(a(2,:)), count(a(:,j) > 3.5)
print *, (a(i,j), i=1,2), minloc(a(:,j)), maxloc(a), shape(a), findloc(a(1,:), 3.)
print *, (i, i=1,1), matmul(a(:,1:2), [1., 1.]), transpose(a(:,1:2)), sum(a, dim=1)
print *, (i, i=1,1), pack(a, a > 2.5)
print *, (i, i=1,2), minloc(a(:,i))
end
""")
        # Array results of functions that are not elemental, MINLOC(a(:,i))
        # after the implied-DO has left i = 3, and PACK of a run-time length.
        expected = ("3.0 4.0 7.0\n1 2 5.0 17.0\n1 3.0 4.0 2.0 4.0 6.0\n"
                    "1 7.0 9.0 1.7320508 2.236068 20.0 40.0\n1 6 48.0 1\n"
                    "3.0 4.0 1 2 3 2 3 2\n1 4.0 6.0 1.0 3.0 2.0 4.0 3.0 7.0 11.0\n"
                    "1 3.0 4.0 5.0 6.0\n1 2 1\n")
        for cmd in build_commands(source, self.directory):
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            actual = [[float(x) for x in s.split()] for s in result.stdout.splitlines()]
            wanted = [[float(x) for x in s.split()] for s in expected.splitlines()]
            self.assertEqual([len(s) for s in actual], [len(s) for s in wanted], (cmd, result.stdout))
            for a, w in zip(sum(actual, []), sum(wanted, [])):
                self.assertAlmostEqual(a, w, places=5, msg=(cmd, result.stdout))

    def test_runtime_controls(self):
        source = self.source("integer :: i,n,step\nread(*,*) n\nread(*,*) step\nwrite(*,*) (i,i=1,n,step),i\nend\n")
        for cmd in build_commands(source, self.directory):
            for data, expected in [("4 1", "1 2 3 4 5"), ("4 2", "1 3 5"), ("0 1", "1"), ("-3 -2", "1 -1 -3 -5")]:
                result = subprocess.run(cmd, input=data.replace(" ","\n")+"\n", capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), expected)
            result = subprocess.run(cmd, input="4\n0\n", capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("step must not be zero", result.stderr)

    def test_dependent_nested_bounds_and_subroutine(self):
        source = self.source("call show(3)\ncontains\nsubroutine show(n)\ninteger,intent(in)::n\ninteger::i,j\nprint *, ((i*10+j,j=1,i),i=1,n),i,j\nend subroutine show\nend\n")
        for cmd in build_commands(source, self.directory):
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "11 21 22 31 32 33 4 4")

    def test_initialization_checks(self):
        source = self.source("integer::a(3),i,n\nread(*,*) n\na(1)=10\na(3)=30\nwrite(*,*) (a(i),i=1,n)\nend\n")
        for cmd in build_commands(source, self.directory):
            result = subprocess.run(cmd, input="1\n", capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "10")
            result = subprocess.run(cmd, input="3\n", capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("uninitialized variable `a`: element 2", result.stderr)
            self.assertIn("example.f90:5", result.stderr)

    def test_invalid_and_unsupported_forms(self):
        for text, message in [
            ("integer::i\nprint *, (i,i=1,3,0)", "step must not be zero"),
            ("real::i\nprint *, (i,i=1,3)", "scalar INTEGER"),
            ("integer::i\nprint *, ((i,i=1,2),i=1,2)", "reuse active variable"),
            ("integer::i,n\nprint *, (i,i=1,n)", "uninitialized variable `n`"),
            ("integer::i\nprint *, (i,i=1.5,3)", "scalar INTEGER expressions"),
            ("integer::i,x\nx=(i,i=1,3)", "array constructor"),
            ("integer::i\nprint *, 2+(i,i=1,3)", "direct list-directed output item"),
        ]:
            with self.subTest(text=text):
                source = self.source(text+"\nend\n")
                result = subprocess.run([COMPILER,"emit-obj",source,self.directory/"bad.obj"],capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)

    def test_increment_overflow(self):
        for first, last, step in [("9223372036854775806_8","9223372036854775807_8","2"), ("-9223372036854775807_8","-9223372036854775807_8","-2")]:
            source=self.source(f"integer(kind=8)::i\nprint *, (i,i={first},{last},{step})\nend\n")
            for cmd in build_commands(source,self.directory):
                result=subprocess.run(cmd,capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0)
                self.assertIn("control variable overflow",result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
