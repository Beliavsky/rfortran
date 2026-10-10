"""Empty and LOGICAL multidimensional arrays in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class EmptyMatrices(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/94_empty_logical_matrices.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="empty-matrices-") as tmp:
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

    def test_invalid_access(self):
        with tempfile.TemporaryDirectory(prefix="empty-matrix-access-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for dims,subscript,dimension in [("0,3","1,1",1),("2,0","1,1",2)]:
                source.write_text("integer::a("+dims+")\nprint *,a("+subscript+")\nend\n")
                result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0,result.stderr)
                self.assertIn("dimension "+str(dimension),result.stderr)
            source.write_text("integer::a(0,3),i\ni=1\nprint *,a(i,1)\nend\n")
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                self.assertIn("dimension 1: subscript outside 1:0",result.stderr)

if __name__=="__main__":
    unittest.main()
