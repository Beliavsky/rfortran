"""Array bounds inquiries: shape-only access, keywords, and invalid arguments."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class BoundInquiries(unittest.TestCase):
    def test_shapes_and_reference(self):
        fixture=ROOT/"tests/82_bound_inquiries.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="bound-inquiries-") as tmp:
            directory=Path(tmp)
            commands=build_commands(fixture,directory)
            if shutil.which("gfortran"):
                exe=directory/"reference.exe"
                result=subprocess.run(["gfortran",fixture,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertEqual(result.stdout.split(),expected.split())

    def test_diagnostics(self):
        cases=[
            ("lbound(a,0)","DIM must be between"),
            ("ubound(a,3)","DIM must be between"),
            ("lbound(a,1.0)","constant INTEGER"),
            ("ubound(a,d)","constant INTEGER"),
            ("ubound(d)","requires an array"),
            ("lbound(a(1,1))","requires an array"),
            ("lbound(a,dim=1,dim=2)","duplicate argument"),
            ("ubound(array=a,1)","positional argument after keyword"),
            ("lbound(a,bad=1)","unknown keyword"),
            ("ubound(dim=1)","requires ARRAY"),
            ("lbound(a,kind=8)","KIND=4"),
            ("ubound(a,1,4,2)","expects ARRAY"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-inquiries-") as tmp:
            path=Path(tmp)/"main.f90"
            for expression,message in cases:
                with self.subTest(expression=expression):
                    path.write_text("program main\ninteger::a(2,3),d\nprint *,"+expression+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_inquiry_does_not_initialize_array(self):
        with tempfile.TemporaryDirectory(prefix="unset-inquiry-") as tmp:
            path=Path(tmp)/"main.f90"
            path.write_text("program main\ninteger::a(2)\nprint *,lbound(a),ubound(a)\nprint *,a\nend program main\n")
            result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True,timeout=15)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("uninitialized",result.stderr)

if __name__ == "__main__":
    unittest.main(verbosity=2)
