"""Explicit constant array bounds, sections, inquiries and argument rebasing."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class ExplicitBounds(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/89_explicit_bounds.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="explicit-bounds-") as tmp:
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
            ("integer::a(-2:2)\na=1\nprint *,a(-3)","subscript -3 outside -2:2"),
            ("integer::a(0:1,-1:1)\na=1\nprint *,a(2,-1)","dimension 1: subscript 2 outside 0:1"),
            ("integer::a(-2:2)\na=1\nprint *,a(-3:-1)","section outside -2:2"),
            ("integer::a(-2:2)\na=1\nprint *,a(::0)","stride must not be zero"),
            ("integer::a(-2:2)\na=1\nprint *,a(1.5)","scalar INTEGER"),
            ("integer::n,a(-2:n)","constant INTEGER"),
            ("integer::a(-9223372036854775807:9223372036854775807)","extent overflows"),
            ("integer::a(-2:2)\nprint *,lbound(a,2)","DIM must be between 1 and 1"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-bounds-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text("program main\n"+body+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_runtime_checks_original_bounds(self):
        cases=[
            ("a(0:1,-1:1)","i=2", "a(i,-1)","dimension 1: subscript outside 0:1"),
            ("a(-2:2)","i=-9223372036854775807", "a(i)","subscript outside -2:2"),
        ]
        with tempfile.TemporaryDirectory(prefix="bounds-error-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for decl,assignment,access,message in cases:
                source.write_text("integer::"+decl+",i\na=1\n"+assignment+"\nprint *,"+access+"\nend\n")
                for command in build_commands(source,directory):
                    with self.subTest(access=access,command=command):
                        result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
