"""Intrinsic module kind constants in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class IntrinsicEnvironmentTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/109_intrinsic_env.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="intrinsic-env-") as tmp:
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

    def test_module_selection_and_host_association(self):
        cases = [
            ("""UsE, &
 & InTrInSiC :: ISO_FORTRAN_ENV, OnLy: INT32
print *,int32
end
""", "4"),
            ("""use iso_fortran_env
integer(int64)::x
integer::intrinsic,non_intrinsic
x=9
intrinsic=2
non_intrinsic=3
print *,kind(x),real128<0,intrinsic+non_intrinsic,x
end
""", "8 T 5 9"),
            ("""program main
use, intrinsic :: iso_fortran_env, only: int64
print *,f()
contains
integer function f()
integer(int64)::x
x=9
f=kind(x)+x
end function f
end program main
""", "17"),
            ("""module iso_fortran_env
integer,parameter::int64=4
end module iso_fortran_env
program main
use, non_intrinsic :: iso_fortran_env, only: int64
print *,int64
end program main
""", "4"),
            ("""module iso_fortran_env
integer,parameter::int64=4
end module iso_fortran_env
program main
use, intrinsic :: iso_fortran_env, only: int64
print *,int64
end program main
""", "8"),
            ("""module iso_fortran_env
integer,parameter::int64=4
end module iso_fortran_env
program main
use :: iso_fortran_env, only: int64
print *,int64
end program main
""", "4"),
        ]
        with tempfile.TemporaryDirectory(prefix="intrinsic-env-selection-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for program,expected in cases:
                source.write_text(program)
                # Run in the temporary directory, so the user module named
                # ISO_FORTRAN_ENV is not saved where other tests would find it.
                for command in build_commands(source,directory,cwd=directory):
                    with self.subTest(program=program,command=command):
                        result=subprocess.run(command,capture_output=True,text=True,timeout=15,cwd=directory)
                        self.assertEqual(result.returncode,0,result.stderr)
                        self.assertEqual(result.stderr,"")
                        self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("use, intrinsic :: missing", "intrinsic module `missing` is not yet supported"),
            ("use, non_intrinsic :: iso_fortran_env", "module interface `iso_fortran_env.rmod` not found"),
            ("use, typo :: iso_fortran_env", "unknown USE module nature"),
            ("use, intrinsic :: iso_fortran_env, only: bogus", "not a public entity"),
            ("use iso_fortran_env, only: int32\ninteger(int64)::x\nx=1\nprint *,x", "not accessible"),
            ("use iso_fortran_env, only: real128\nreal(real128)::x\nx=1\nprint *,x", "unsupported REAL kind -1"),
        ]
        with tempfile.TemporaryDirectory(prefix="intrinsic-env-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
