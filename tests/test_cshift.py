"""CSHIFT in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class CircularShift(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/100_cshift.f90"
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

    def test_large_shifts_and_kinds(self):
        program="""program main
integer(8)::s,a(3)
double precision::d(3)
s=huge(s)
a=[10,20,30]
d=[1.0d0,2.0d0,3.0d0]
print *,cshift(a,s)
s=-s-1
print *,cshift(a,s)
print *,cshift(d,-1)
print *,cshift(cshift([1,2,3],1),-1)
end program main
"""
        with tempfile.TemporaryDirectory(prefix="cshift-kinds-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"20 30 10\n20 30 10\n3.0 1.0 2.0\n1 2 3\n"),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("print *,cshift([1,2],1,0)", "DIM must be between"),
            ("print *,cshift([1,2],1,2)", "DIM must be between"),
            ("integer::d\nd=1\nprint *,cshift([1,2],1,d)", "constant INTEGER"),
            ("print *,cshift([1,2],1.5)", "must be INTEGER"),
            ("print *,cshift([1,2],[1])", "SHIFT shape"),
            ("print *,cshift(1,1)", "must be an array"),
            ("print *,cshift([1,2])", "requires SHIFT"),
            ("print *,cshift([1,2],1,shift=2)", "duplicate"),
            ("print *,cshift([1,2],1,unknown=2)", "unknown keyword"),
            ("integer::a(2,3)\na=cshift(reshape([1,2,3,4,5,6],[3,2]),1)", "nonconforming"),
        ]
        with tempfile.TemporaryDirectory(prefix="cshift-diagnostics-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()

