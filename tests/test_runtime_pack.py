"""Runtime PACK in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class RuntimePackingTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/107_runtime_pack.f90"
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

    def test_shape_errors(self):
        cases=[
            "print *,all(pack(a,mask)==[1])",
            "print *,all([1]==pack(a,mask))",
            "v=pack(a,mask)",
            "print *,all(pack(a,mask)==pack(a,a>1))",
        ]
        with tempfile.TemporaryDirectory(prefix="runtime-pack-shape-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for operation in cases:
                source.write_text("integer::a(3),v(1)\nlogical::mask(3)\na=[1,2,3]\nmask=.true.\n"+operation+"\nprint *,v\nend\n" if operation.startswith("v=") else "integer::a(3)\nlogical::mask(3)\na=[1,2,3]\nmask=.true.\n"+operation+"\nend\n")
                for command in build_commands(source,directory):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn("runtime PACK result shape does not conform",result.stdout+result.stderr)

    def test_selected_reads_and_empty_source(self):
        program="""program main
integer::a(4),empty(0)
logical::mask(4),inactive
mask=[.false.,.true.,.false.,.true.]
a(2)=7
a(4)=9
print *,pack(a,mask),sum(pack(a,mask))
inactive=.false.
print *,pack(a,inactive),sum(pack(empty,inactive))
end program main
"""
        with tempfile.TemporaryDirectory(prefix="runtime-pack-selected-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"7 9 16\n0\n"),result.stdout)

    def test_unsupported_consumers(self):
        operations=[
            "print *,cshift(pack(a,mask),1)",
            "print *,spread(pack(a,mask),1,2)",
            "print *,sum([pack(a,mask)])",
            "print *,sum(pack(a,mask),dim=1)",
            "print *,unpack(pack(a,mask),mask,0)",
            "print *,pack(a,mask,vector=pack(a,mask))",
            "print *,ubound(pack(a,mask))",
        ]
        with tempfile.TemporaryDirectory(prefix="runtime-pack-unsupported-") as tmp:
            source=Path(tmp)/"main.f90"
            for operation in operations:
                source.write_text("integer::a(3)\nlogical::mask(3)\na=1\nmask=.true.\n"+operation+"\nend\n")
                result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0,operation)
                self.assertIn("runtime PACK",result.stderr)

if __name__=="__main__":
    unittest.main()
