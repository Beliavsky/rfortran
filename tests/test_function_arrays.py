"""Fixed array arguments to pure functions in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class FunctionArrays(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/92_function_arrays.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="function-arrays-") as tmp:
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
            ("integer::a(3)\na=1", "a", "integer,intent(in)::x(3)", False, "requires PURE"),
            ("integer::a(3)\na=1", "a", "integer,intent(in)::x(:)", True, "fixed constant extents"),
            ("integer::a(2)\na=1", "a", "integer,intent(in)::x(3)", True, "type or shape mismatch"),
            ("real::a(3)\na=1.0", "a", "integer,intent(in)::x(3)", True, "type or shape mismatch"),
            ("integer::a(3)\na=1", "a(:)", "integer,intent(in)::x(3)", True, "whole-array actual"),
            ("integer::a(3)\na=1", "a", "integer,intent(in),optional::x(3)", True, "OPTIONAL/ALLOCATABLE"),
            ("integer::a(1025)\na=1", "a", "integer,intent(in)::x(1025)", True, "at most 1024"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-function-arrays-") as tmp:
            source=Path(tmp)/"main.f90"
            for decl,actual,dummy,pure,message in cases:
                source.write_text("program main\n"+decl+"\nprint *,f("+actual+")\ncontains\n"+("pure " if pure else "")+"integer function f(x) result(n)\n"+dummy+"\nn=sum(x)\nend function f\nend program main\n")
                with self.subTest(dummy=dummy,actual=actual):
                    result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                    self.assertIn(message,result.stderr)

    def test_input_initialization(self):
        # Copying the input must retain the caller's uninitialized-element checks.
        with tempfile.TemporaryDirectory(prefix="function-array-unset-") as tmp:
            source=Path(tmp)/"main.f90"
            source.write_text("integer::a(3)\na(1)=1\nprint *,f(a)\ncontains\npure integer function f(x) result(n)\ninteger,intent(in)::x(3)\nn=sum(x)\nend function f\nend\n")
            for mode in ("run", "emit-obj"):
                command=[COMPILER,mode,source]
                if mode=="emit-obj": command.append(Path(tmp)/"main.obj")
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                self.assertIn("uninitialized",result.stderr)

if __name__=="__main__":
    unittest.main()
