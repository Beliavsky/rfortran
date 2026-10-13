"""Main-program declaration initialization in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class MainInitializationTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/112_main_initialization.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="main-init-") as tmp:
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

    def test_constants_and_partial_declarations(self):
        program="""integer,parameter::n=3
integer::i,a(3)=[(i*i,i=1,n)],x=4,y
INTEGER::UPPER=8,UPARRAY(2)=[9,10]
real::r=1.0+n
character(3)::s='abcdef',t='a'//'b'
y=2
x=x+y
print *,a,x,y,r,'[',s,'][',t,']'
PRINT *,UPPER,UPARRAY
end
"""
        with tempfile.TemporaryDirectory(prefix="main-init-constants-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertTrue(equivalent(result.stdout,"1 4 9 6 2 4.0 [abc][ab ]\n8 9 10"),result.stdout)

    def test_diagnostics_and_persistent_storage_boundary(self):
        cases=[
            ("integer::x=1,y=x\nprint *,y\nend", "invalid declaration initializer for `y`"),
            ("integer::x=missing\nprint *,x\nend", "invalid declaration initializer"),
            ("integer(1)::x=128\nprint *,x\nend", "invalid declaration initializer"),
            ("integer::x=[1,2]\nprint *,x\nend", "scalar declaration initializer cannot be an array"),
            ("integer::x(2)=[1,2,3]\nprint *,x\nend", "nonconforming array assignment"),
            ("module m\ninteger::x=1\nend module m\nend", "module storage"),
            ("block\ninteger::x=0\nprint *,x\nend block\nend", "module storage"),
        ]
        with tempfile.TemporaryDirectory(prefix="main-init-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for program,message in cases:
                with self.subTest(program=program):
                    source.write_text(program+"\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
