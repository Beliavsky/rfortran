"""Integer literal kinds and exponentiation in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class IntegerLiteralTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/111_integer_literals.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="integer-literals-") as tmp:
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

    def test_constant_contexts_and_unevaluated_kind(self):
        program="""program main
integer,parameter::wide_kind=8,n=2_wide_kind+1_1
integer(8_1)::unset,a(n)
integer::exponent
real::single
a=[4_8,5_8,6_8]
exponent=-3
single=2.0
print *,kind(unset+1_1),kind(1_1+unset)
print *,kind(single**1_8),kind(2_1**unset)
print *,a(1_8),a(3_1),sum(a)
print *,2**exponent,(-1)**exponent,-2**2,(-2)**2,2**3**2
print *,kind(+1_8),kind(-1_2),kind(1_8/2_1),kind(abs(-1_8))
end program main
"""
        with tempfile.TemporaryDirectory(prefix="integer-constants-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertTrue(equivalent(result.stdout,"8 8\n4 8\n4 6 15\n0 -1 -4 4 512\n8 2 8 8"),result.stdout)
                    self.assertNotIn("Runtime error",result.stderr)

    def test_diagnostics(self):
        cases=[
            ("128_1", "out of range for kind 1"),
            ("32768_2", "out of range for kind 2"),
            ("2147483648_4", "out of range for kind 4"),
            ("9223372036854775808_8", "64-bit range"),
            ("1_3", "unsupported integer literal kind 3"),
            ("1_16", "unsupported integer literal kind 16"),
            ("1_missing", "not a previously defined integer PARAMETER"),
        ]
        with tempfile.TemporaryDirectory(prefix="integer-literal-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for expression,message in cases:
                with self.subTest(expression=expression):
                    source.write_text("print *,"+expression+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
