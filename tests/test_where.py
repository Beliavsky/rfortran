"""WHERE masks, simultaneous assignments, logical arrays and diagnostics."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class WhereTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/88_where.f90"
        expected=json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="where-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout,expected)
                    self.assertEqual(result.stderr,"")
            if shutil.which("gfortran"):
                exe=directory / "reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],cwd=directory,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(expected,result.stdout),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("integer::a(2)\na=1", "where(.true.) a=2", "mask must be an array"),
            ("integer::a(2)\na=1", "where(a) a=2", "LOGICAL array"),
            ("integer::a(2),b(3)\na=1\nb=2", "where(b>0) a=2", "shape does not conform"),
            ("integer::a(2)\na=1", "where(a>0) a=.true.", "incompatible numeric/LOGICAL"),
            ("integer::a(2)\na=1", "where(a>0) a(:)=2", "section destinations"),
            ("integer::a(2),b(3)\na=1\nb=2", "where(a>0)\na=3\nelsewhere(b>0)\na=4\nendwhere", "mask shapes do not conform"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-where-") as tmp:
            source=Path(tmp)/"main.f90"
            for decl,body,message in cases:
                with self.subTest(body=body):
                    source.write_text("program main\n"+decl+"\n"+body+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_selected_uninitialized_element(self):
        with tempfile.TemporaryDirectory(prefix="unset-where-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text("integer::a(2),b(2)\na=1\nb(1)=7\nwhere(a>0) a=b\nprint *,a\nend\n")
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn("uninitialized variable `b`",result.stderr)
                    self.assertIn("element 2",result.stderr)

if __name__=="__main__":
    unittest.main()
