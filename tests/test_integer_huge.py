"""Integer HUGE and KIND inquiries in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class IntegerHuge(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/93_integer_huge.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="integer-huge-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout,expected)
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
            ("integer(kind=16)::i\nprint *,huge(i)","HUGE currently supports INTEGER kinds"),
            ("logical::flag\nprint *,huge(flag)","INTEGER or REAL"),
            ("integer::i\nprint *,tiny(i)","REAL argument"),
            ("print *,tiny(0)","REAL argument"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-huge-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                source.write_text("program main\n"+body+"\nend program main\n")
                with self.subTest(body=body):
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                    self.assertIn(message,result.stderr)

    def test_undefined_argument_is_not_read(self):
        with tempfile.TemporaryDirectory(prefix="huge-unset-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text("integer(kind=8)::x\nprint *,huge(x),kind(huge(x))\nend\n")
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout," 9223372036854775807 8\n")
                self.assertNotIn("uninitialized",result.stderr)

if __name__=="__main__":
    unittest.main()
