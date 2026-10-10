"""SELECT CASE ranges and expression selectors in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class CaseRanges(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/97_case_ranges.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="case-ranges-") as tmp:
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
            ("integer::i\ni=2\nselect case(i)\ncase(1:3,3:5)\nprint *,1\nend select", "overlapping"),
            ("integer::i,n\ni=2\nn=3\nselect case(i)\ncase(1:n)\nprint *,1\nend select", "constant INTEGER"),
            ("integer::i\ni=2\nselect case(i)\ncase(1::3)\nprint *,1\nend select", "unexpected"),
            ("character::c\nc='a'\nselect case(c)\ncase('a':'z')\nprint *,1\nend select", "scalar INTEGER"),
        ]
        with tempfile.TemporaryDirectory(prefix="case-diagnostics-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
