"""RESHAPE and array constructors in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class ReshapeOptions(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/102_reshape_options.f90"
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
            ("print *,reshape([1,2],[2,2],order=[1,1])", "permutation"),
            ("print *,reshape([1,2],[2,2],order=[1])", "permutation"),
            ("print *,reshape([1,2],[2,2],order=[2,3])", "permutation"),
            ("print *,reshape([1,2],[2],order=[1.0])", "constant INTEGER"),
            ("integer::ordering(1)\nordering=1\nprint *,reshape([1,2],[2],order=ordering)", "constant array constructor"),
            ("integer::empty(0)\nprint *,reshape([1],[2],pad=empty)", "nonempty PAD"),
            ("print *,reshape([1],[2],pad=2)", "PAD must be an array"),
            ("print *,reshape([1],[2],pad=[2.0])", "same type and kind"),
            ("print *,reshape([1],[2],pad=[2],order=[1],pad=[3])", "duplicate"),
            ("print *,reshape([1],[2],[2],[1],[3])", "too many arguments"),
        ]
        with tempfile.TemporaryDirectory(prefix="reshape-options-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_unused_padding_and_pure(self):
        program="""program main
integer::empty(0),a(2)
a=reshape([4,5,6],[2],pad=empty)
print *,a,f()
contains
pure integer function f()
integer::b(2,2)
b=reshape([1],[2,2],pad=[2,3],order=[2,1])
f=sum(b)
end function f
end program main
"""
        with tempfile.TemporaryDirectory(prefix="reshape-options-pure-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"4 5 8\n"),result.stdout)

if __name__=="__main__":
    unittest.main()
