"""Contextual DOUBLE PRECISION words in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class DoublePrecisionNamesTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/110_double_precision_names.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="double-context-") as tmp:
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

    def test_procedure_names_and_continuations(self):
        programs=[("""program main
print *,double(3),precision(4)
contains
integer function double(precision)
integer,intent(in)::precision
double=2*precision
end function double
integer function precision(double)
integer,intent(in)::double
precision=double+1
end function precision
end program main
""", "6 5"),
        ("""program main
DoUbLe & ! type phrase continues
 & PrEcIsIoN :: double,precision
double=1.25d0
precision=2.5d0
print *,double,precision,kind(double),kind(precision)
end program main
""", "1.25 2.5 8 8")]
        with tempfile.TemporaryDirectory(prefix="double-context-procedures-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for program,expected in programs:
                source.write_text(program)
                for command in build_commands(source,directory):
                    with self.subTest(program=program,command=command):
                        result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                        self.assertEqual(result.returncode,0,result.stderr)
                        self.assertEqual(result.stderr,"")
                        self.assertTrue(equivalent(result.stdout,expected),result.stdout)

if __name__=="__main__":
    unittest.main()
