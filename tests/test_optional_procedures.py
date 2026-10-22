"""OPTIONAL procedure specialization and scalar CHARACTER arguments in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent
import test_deallocate

class OptionalProcedureTests(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_optional_derived_dummy(self):
        """An OPTIONAL derived-type dummy, passed, omitted and forwarded, as in
        statistics.f90 display_stretch_param."""
        self.check('module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ntype :: stretch_param\n   real(dp) :: x1, x2, power=1.0_dp\n   character (len=10) :: normalization\nend type stretch_param\ncontains\nsubroutine display(par,label)\ntype(stretch_param), intent(in), optional :: par\ncharacter(len=*), intent(in), optional :: label\nif (present(label)) print \'(a)\', label\nif (.not. present(par)) then\n   print \'(a)\', "no par"\n   return\nend if\nprint \'(3f8.3,1x,a)\', par%x1, par%x2, par%power, trim(par%normalization)\nend subroutine\nsubroutine stretch(par)\ntype(stretch_param), intent(in) :: par\ncall display(par, label="in stretch")\nend subroutine\nend module\nuse m\ntype(stretch_param) :: p\np = stretch_param(1.0_dp, 2.0_dp, 3.0_dp, "sd")\ncall display(p)\ncall display()\ncall display(label="only label")\ncall stretch(p)\nend\n','   1.000   2.000   3.000 sd\nno par\nonly label\nno par\nin stretch\n   1.000   2.000   3.000 sd\n')

    def test_conditional_references_to_absent_arguments(self):
        """A reference to an absent OPTIONAL dummy that may not execute is
        an error only when it does, as in statistics.f90 stat_vec_str_many."""
        program="""module m
implicit none
contains
function lags(x,n) result(y)
real, intent(in) :: x(:)
integer, intent(in) :: n
real :: y(n)
y = x(1)
end function
function stats(x,nacf) result(y)
real, intent(in) :: x(:)
integer, intent(in), optional :: nacf
real, allocatable :: y(:)
integer :: n
n = 0
if (present(nacf)) n = nacf
allocate(y(n+1))
y(1) = sum(x)
if (n > 0) y(2:n+1) = lags(x,nacf)
end function
integer function next(k,stride)
integer, intent(in) :: k
integer, intent(in), optional :: stride
next = k
if (k < 0) return
next = k + stride
end function
end module
use m
print *, stats([1.,2.],2)
print *, stats([1.,2.])
print *, next(1,2)
print *, next(1)
end
"""
        self.check(program.replace("print *, next(1)\n",""),"3.0 1.0 1.0\n3.0\n3\n")
        self.check(program,error="absent OPTIONAL dummy `stride`")

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
