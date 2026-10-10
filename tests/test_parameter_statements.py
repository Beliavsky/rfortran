"""Separate PARAMETER statements in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class ParameterStatements(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/98_parameter_statements.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="parameter-statements-") as tmp:
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
            ("integer::i\nparameter(i=1)\ni=2", "cannot modify PARAMETER"),
            ("integer::i\nparameter(i=1,i=2)", "duplicate PARAMETER"),
            ("integer::i,j\nparameter(i=j,j=2)", "invalid initializer"),
            ("parameter(i=1)", "preceding scalar type declaration"),
            ("integer::a(2)\nparameter(a=[1,2])", "preceding scalar type declaration"),
            ("integer::i\nparameter(i=i+1)", "invalid initializer"),
        ]
        with tempfile.TemporaryDirectory(prefix="parameter-diagnostics-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)
            source.write_text("subroutine s(i)\ninteger::i\nparameter(i=1)\nend subroutine s\n")
            result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],capture_output=True,text=True,timeout=15)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("dummy argument",result.stderr)

if __name__=="__main__":
    unittest.main()
