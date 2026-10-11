"""OPTIONAL procedure specialization and scalar CHARACTER arguments in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class OptionalProcedureTests(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/113_optional_procedures.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="optional-procedures-") as tmp:
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

    def test_results_returns_and_character_forwarding(self):
        program="""program main
character(4)::text
print *,f(),f(5),g(),g(6)
call outer()
call outer('abc')
call fill(text)
print *,'[',text,']'
contains
pure integer function f(x)
integer,intent(in),optional::x
if (.not.present(x)) then
f=11
return
end if
f=x
end function f
function g(x) result(y)
integer,intent(in),optional::x
integer::y
y=12
if(present(x)) y=x
end function g
subroutine outer(text)
character(*),intent(in),optional::text
call inner(text)
end subroutine outer
subroutine inner(text)
character(*),intent(in),optional::text
if (present(text)) then
print *,text,len(text)
else
print *,'absent'
end if
end subroutine inner
subroutine fill(dest)
character(*),intent(out)::dest
dest='yes'
end subroutine fill
end program main
"""
        with tempfile.TemporaryDirectory(prefix="optional-results-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertTrue(equivalent(result.stdout,"11 5 12 6\nabsent\nabc 3\n[yes ]"),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("print *,f()", "integer function f(x)\ninteger,optional,intent(out)::x\nf=1\nend function f", "requires INTENT(IN)"),
            ("call s()", "subroutine s(x)\ncharacter(*),optional,intent(in)::x\nprint *,x\nend subroutine s", "absent OPTIONAL"),
            ("call s()", "subroutine s(x)\ncharacter(*),optional,intent(out)::x\nx='a'\nend subroutine s", "absent OPTIONAL"),
            ("call s('a')", "subroutine s(x)\ncharacter(*),intent(in)::x\nx='b'\nend subroutine s", "INTENT(IN)"),
            ("call s(1)", "subroutine s(x)\ncharacter(*),intent(in)::x\nprint *,x\nend subroutine s", "type mismatch"),
            ("call s('ab')", "subroutine s(x)\ncharacter(3),intent(in)::x\nprint *,x\nend subroutine s", "length mismatch"),
            ("call s()", "subroutine s(x)\ncharacter(*),intent(in)::x\nprint *,x\nend subroutine s", "missing required"),
            ("print *,f()", "integer function f(x)\ninteger,optional,intent(in)::x\nf=x\nend function f", "absent OPTIONAL"),
            ("print *,f(1)", "integer function f(x,y)\ninteger,intent(in)::x\ninteger,optional,intent(in)::y\nf=0\nif(present(x)) f=1\nend function f", "OPTIONAL dummy"),
            ("print *,f()", "integer function f(x)\ninteger,optional,intent(in)::x\nf=g(x)\nend function f\ninteger function g(x)\ninteger,intent(in)::x\ng=x\nend function g", "absent OPTIONAL"),
        ]
        with tempfile.TemporaryDirectory(prefix="optional-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for call,body,message in cases:
                with self.subTest(call=call,body=body):
                    source.write_text("program main\n"+call+"\ncontains\n"+body+"\nend program main\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"main.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()
