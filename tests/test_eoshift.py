"""EOSHIFT in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class EndOffShifts(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/104_eoshift.f90"
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
            ("print *,eoshift(1,1)", "ARRAY must be an array"),
            ("print *,eoshift([1,2])", "requires SHIFT"),
            ("print *,eoshift([1,2],1,dim=0)", "DIM must be between"),
            ("integer::dim\ndim=1\nprint *,eoshift([1,2],1,dim=dim)", "constant INTEGER"),
            ("print *,eoshift([1,2],1.0)", "SHIFT must be INTEGER"),
            ("print *,eoshift([1,2],1,2.0)", "same type and kind"),
            ("print *,eoshift([1,2],1,boundary=[3])", "BOUNDARY shape"),
            ("integer::a(2,3)\na=1\nprint *,eoshift(a,[1,2],dim=1)", "SHIFT shape"),
            ("integer::a(2,3,4)\na=1\nprint *,eoshift(a,1,reshape([1,2,3,4,5,6,7,8],[4,2]),dim=2)", "BOUNDARY shape"),
            ("print *,eoshift([1,2],1,unknown=2)", "unknown keyword"),
            ("print *,eoshift([1,2],1,shift=2)", "duplicate"),
        ]
        with tempfile.TemporaryDirectory(prefix="eoshift-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_extreme_shifts_and_masked_assignment(self):
        program="""program main
integer(8)::shift(2),a(2,3),boundary
integer::v(4)
a=7
boundary=9
shift(1)=huge(shift)
shift(2)=-huge(shift)-1
print *,eoshift(a,shift,boundary,2)
print *,eoshift(a,shift(2),dim=2)
v=[1,2,3,4]
where(v>1)
 v=eoshift(v,-1,8)
end where
print *,v
end program main
"""
        with tempfile.TemporaryDirectory(prefix="eoshift-extreme-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"9 9 9 9 9 9\n0 0 0 0 0 0\n1 1 2 3\n"),result.stdout)

if __name__=="__main__":
    unittest.main()
