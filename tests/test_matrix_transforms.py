"""RESHAPE and array constructors in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class MatrixTransforms(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/103_matrix_transforms.f90"
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
            ("print *,transpose(1)", "rank-two array"),
            ("print *,transpose([1,2])", "rank-two array"),
            ("print *,transpose(reshape([1,2],[1,1,2]))", "rank-two array"),
            ("print *,transpose()", "one MATRIX"),
            ("print *,transpose(array=[1,2])", "unknown keyword"),
            ("integer::a(2,3),b(2,3)\na=1\nb=transpose(a)", "nonconforming"),
            ("integer::a(2,3)\na=1\nprint *,cshift(a,[1,2],dim=1)", "SHIFT shape"),
            ("integer::a(2,3,4)\na=1\nprint *,cshift(a,reshape([1,2,3,4,5,6,7,8],[4,2]),dim=2)", "SHIFT shape"),
            ("integer::a(2,3)\na=1\nprint *,cshift(a,[1.0,2.0],dim=2)", "must be INTEGER"),
        ]
        with tempfile.TemporaryDirectory(prefix="matrix-transform-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_kinds(self):
        program="""program main
integer(8)::shifts(2),a(2,3)
double precision::b(2,3)
a=reshape([1,2,3,4,5,6],[2,3])
b=a
shifts(1)=huge(shifts)
shifts(2)=-huge(shifts)-1
print *,cshift(a,shifts,2)
print *,transpose(b)
end program main
"""
        with tempfile.TemporaryDirectory(prefix="matrix-transform-kinds-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"3 4 5 6 1 2\n1 3 5 2 4 6\n"),result.stdout)

if __name__=="__main__":
    unittest.main()
