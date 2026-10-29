"""Masked numeric reductions in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class MaskedReductions(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/90_masked_reductions.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="masked-reductions-") as tmp:
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
            ("integer::a(3)\na=1\nprint *,sum(a,dim=0)","DIM must be between"),
            ("integer::a(2,3),b(3,2)\na=1\nb=1\nprint *,sum(a,mask=b>0)","MASK shape"),
            ("integer::a(2,3)\na=1\nprint *,sum(a,dim=3)","DIM must be between"),
            ("integer::a(3),m(3)\na=1\nm=1\nprint *,sum(a,mask=m)","MASK must be LOGICAL"),
            ("integer::a(3)\nlogical::m(2)\na=1\nm=.true.\nprint *,sum(a,mask=m)","MASK shape"),
            ("integer::a(3)\na=1\nprint *,sum(a,array=a)","duplicate argument"),
            ("integer::a(3)\na=1\nprint *,sum(array=a,1)","positional argument after"),
            ("integer::a(3)\na=1\nprint *,sum(a,foo=1)","unknown keyword"),
            ("integer::a(3)\na=1\nprint *,sum(mask=.true.)","requires ARRAY"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-reductions-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text("program main\n"+body+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)


if __name__=="__main__":
    unittest.main()
