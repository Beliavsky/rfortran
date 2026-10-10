"""SPREAD in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class SpreadTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/105_spread.f90"
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
            ("print *,spread(1,0,2)", "DIM must be between"),
            ("print *,spread(1,2,2)", "DIM must be between"),
            ("print *,spread([1,2],3,2)", "DIM must be between"),
            ("integer::d\nd=1\nprint *,spread(1,d,2)", "DIM must currently be a constant INTEGER"),
            ("integer::n\nn=2\nprint *,spread(1,1,n)", "NCOPIES must currently be a constant INTEGER"),
            ("print *,spread(1,1,2.0)", "NCOPIES must currently be a constant INTEGER"),
            ("print *,spread(1,1)", "requires NCOPIES"),
            ("print *,spread(dim=1,ncopies=2)", "requires SOURCE"),
            ("print *,spread(1,1,2,dim=1)", "duplicate"),
            ("print *,spread(1,1,2,unknown=1)", "unknown keyword"),
            ("integer::a(2,3)\na=spread([1,2],1,3)", "nonconforming"),
        ]
        with tempfile.TemporaryDirectory(prefix="spread-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_kinds_and_where(self):
        program="""program main
integer(8)::x,a(3)
integer::b(3)
x=huge(x)
a=spread(x,1,3)
print *,all(a==x)
b=[1,2,3]
where(b>1)
 b=spread(b(1),1,3)
end where
print *,b
print *,sum([spread(2,1,3),4])
end program main
"""
        with tempfile.TemporaryDirectory(prefix="spread-kinds-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"T\n1 1 1\n10\n"),result.stdout)

if __name__=="__main__":
    unittest.main()
