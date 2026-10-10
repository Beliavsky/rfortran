"""Logical reductions in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class LogicalReductions(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/91_logical_reductions.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="logical-reductions-") as tmp:
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
            ("logical::m(3)\nm=.true.\nprint *,count(m,dim=0)","DIM must be between"),
            ("integer::a(2,3)\na=1\nprint *,count(a>0,dim=3)","DIM must be between"),
            ("integer::d\nlogical::m(3)\nd=1\nm=.true.\nprint *,any(m,dim=d)","constant INTEGER"),
            ("logical::m(3)\nm=.true.\nprint *,all(m,dim=.true.)","constant INTEGER"),
            ("integer::a(3)\na=1\nprint *,count(a)","incorrect numeric/logical type"),
            ("print *,count(.true.)","must be an array"),
            ("logical::m(3)\nm=.true.\nprint *,count(m,mask=m)","duplicate argument"),
            ("logical::m(3)\nm=.true.\nprint *,count(mask=m,1)","positional argument after"),
            ("logical::m(3)\nm=.true.\nprint *,count(array=m)","unknown keyword"),
            ("print *,count(kind=4)","requires MASK"),
            ("logical::m(3)\nm=.true.\nprint *,count(m,kind=2)","KIND=4 or KIND=8"),
            ("logical::m(3)\ninteger::k\nm=.true.\nk=8\nprint *,count(m,kind=k)","constant INTEGER"),
            ("logical::m(3)\nm=.true.\nprint *,any(m,kind=4)","unknown keyword"),
            ("logical::m(3)\nm=.true.\nprint *,all(m,1,4)","too many arguments"),
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
