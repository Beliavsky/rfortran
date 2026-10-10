"""RESHAPE and array constructors in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class ReshapeConstructors(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/96_reshape_constructors.f90"
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
            ("print *,reshape([1,2],[2,2])", "too few elements"),
            ("print *,reshape([1,2],[-1,2])", "nonnegative"),
            ("print *,reshape([1,2],[2],order=[1])", "PAD and ORDER"),
            ("print *,reshape(shape=[2])", "requires SOURCE"),
            ("print *,reshape([1,2],source=[1,2],shape=[2])", "duplicate"),
            ("print *,sum([1,.true.])", "same type and kind"),
            ("integer::a(2,3),b(3,2)\na=1\nb=2\nprint *,mod(a,b)", "nonconforming"),
            ("integer::a(2)\na=1\nprint *,mod(a,2.0)", "same numeric type"),
            ("integer::a(2,3)\na=reshape([1,2,3,4,5,6],[3,2])", "nonconforming"),
        ]
        with tempfile.TemporaryDirectory(prefix="reshape-diagnostics-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_sections_kinds_and_pure(self):
        program="""program main
integer(4)::a(4)
integer(8)::b(2),c(2,1)
double precision::d(2,1)
a=[1,2,3,4]
a(:)=[a(3:4),a(1:2)]
b=7
c=reshape(b,[2,1])
d=reshape([1.0d0,2.0d0],[2,1])
print *,a,mod(a,2),sum(c),sum(d)
a=[f(),abs(-2),f(),4]
print *,f(),a
contains
pure integer function f()
integer::x(2,2)
x=reshape([1,2,3,4],[2,2])
f=sum(x)
end function f
end program main
"""
        with tempfile.TemporaryDirectory(prefix="reshape-kinds-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"3 4 1 2 1 0 1 0 14 3.0\n10 10 2 10 4\n"),result.stdout)

if __name__=="__main__":
    unittest.main()
