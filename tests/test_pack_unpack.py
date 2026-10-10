"""PACK and UNPACK in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class PackingTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/106_pack_unpack.f90"
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
            ("print *,pack(1,.true.)", "ARRAY must be an array"),
            ("print *,unpack(1,[.true.],0)", "rank-one array"),
            ("print *,unpack([1],.true.,0)", "MASK must be an array"),
            ("print *,pack([1,2],1,[3,4])", "MASK must be LOGICAL"),
            ("print *,pack([1,2],.true.,[3.0,4.0])", "same type and kind"),
            ("print *,unpack([1],[.true.],0.0)", "same type and kind"),
            ("print *,unpack([1],[.true.],[2,3])", "FIELD shape"),
            ("print *,pack([1,2],[.true.])", "MASK shape"),
            ("integer::a(2,3)\na=1\nprint *,pack(a,reshape([.true.,.true.,.true.,.true.,.true.,.true.],[3,2]),[1,2])", "MASK shape"),
            ("print *,unpack([1],[.true.])", "requires FIELD"),
            ("print *,pack([1],.true.,mask=.true.)", "duplicate"),
        ]
        with tempfile.TemporaryDirectory(prefix="packing-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_only_selected_elements_are_read(self):
        program="""program main
integer::a(4),v(3),field(4),empty(0)
logical::mask(4)
mask=[.false.,.true.,.false.,.true.]
a(2)=7
a(4)=8
v(3)=9
print *,pack(a,mask,v)
v(1)=10
v(2)=20
field(1)=1
field(3)=3
print *,unpack(v,mask,field)
print *,sum(unpack(empty,[.false.,.false.],2))
end program main
"""
        with tempfile.TemporaryDirectory(prefix="packing-selected-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"7 8 9\n1 10 3 20\n4\n"),result.stdout)

    def test_short_vector_runtime_error(self):
        with tempfile.TemporaryDirectory(prefix="packing-short-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for expr,name in [("pack([1,2],mask,vector=[3])","PACK"),("unpack([1],mask,0)","UNPACK")]:
                source.write_text("logical::mask(2)\nmask=.true.\nprint *,"+expr+"\nend\n")
                for command in build_commands(source,directory):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(name+" VECTOR has too few elements",result.stdout+result.stderr)

if __name__=="__main__":
    unittest.main()
