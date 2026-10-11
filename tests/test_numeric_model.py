"""Numeric model inquiries and empty list-directed PRINT, checked against gfortran.

Run after cargo build --bin rfort: python tests/test_numeric_model.py
Set RFORT_TEST_COMPILER to test a release compiler.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent


class NumericModel(unittest.TestCase):
    def compare(self, program, expected, records=False, forbidden=()):
        with tempfile.TemporaryDirectory(prefix="numeric-model-") as tmp:
            directory=Path(tmp)
            source=directory / "model.f90"
            source.write_text(program,encoding="utf-8",newline="")
            commands=build_commands(source,directory)
            if shutil.which("gfortran"):
                exe=directory / "reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,
                                      capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result=subprocess.run(command,cwd=directory,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    for text in forbidden:
                        self.assertNotIn(text,result.stdout)
                    if len(command)>1 and command[1]=="run":
                        self.assertNotIn("error:",result.stderr.lower())
                    else:
                        self.assertEqual(result.stderr,"")
                    if records:
                        lines=result.stdout.splitlines()
                        self.assertEqual(len(lines),len(expected.splitlines()))
                        for actual,wanted in zip(lines,expected.splitlines()):
                            self.assertTrue(equivalent(actual,wanted),(actual,wanted))
                    else:
                        self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_supported_models_and_kinds(self):
        source=ROOT / "tests/124_numeric_model.f90"
        expected=json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        self.compare(source.read_text(encoding="utf-8"),expected,records=True)

    def test_empty_print_statement_boundaries(self):
        self.compare("""program main
implicit none
print * ! comment
print *, 'first'; print *; print *, 'second'
if (.true.) print *
if (.false.) print *
print &
 & *
print *, &
 & 'third'
print *
end program
""".replace("\n","\r\n"),"\nfirst\n\nsecond\n\n\nthird\n\n",records=True)

    def test_parameters_scopes_and_pure_procedures(self):
        self.compare("""module models
implicit none
private
public :: dp, x, p
integer,parameter::dp=8
real(dp)::x
integer,parameter::p=digits(x)
end module models
program main
use models,only:dp,x,p
implicit none
integer,parameter::q=precision(x)
integer::d,r
print *,p,q,range(x)
call check(d,r)
print *,d,r
contains
pure subroutine check(dout,rout)
integer,intent(out)::dout,rout
integer::x
integer,parameter::d=digits(x),r=range(x)
dout=d
rout=r
end subroutine check
end program
""","53 15 307\n31 9\n")

    def test_argument_values_and_subscripts_are_not_read(self):
        self.compare("""program main
implicit none
integer::i
real::x,a(-3:3)
double precision::y
print *,digits(a(i)),precision(y+x),range(a(:)),radix(x/x)
print *,epsilon(y)/epsilon(1.0_8),minexponent(a),maxexponent(a)
end program
""","24 15 37 2\n1 -125 128\n")

    def test_invalid_arguments(self):
        for expression,diagnostic in [
            ("epsilon(1)","requires a REAL"),
            ("precision(1)","requires a REAL"),
            ("minexponent(1)","requires a REAL"),
            ("maxexponent(.true.)","requires a REAL"),
            ("digits(.true.)","INTEGER or REAL"),
            ("range('abc')","INTEGER or REAL"),
            ("radix(.false.)","INTEGER or REAL"),
            ("digits(1,2)","argument"),
            ("epsilon()","argument"),
            ("digits(a=1)","unknown keyword"),
            ("radix(x=1,x=2)","duplicate"),
            ("digits(missing)","inquiry argument"),
        ]:
            with self.subTest(expression=expression), tempfile.TemporaryDirectory(prefix="numeric-invalid-") as tmp:
                directory=Path(tmp)
                source=directory / "invalid.f90"
                source.write_text(f"program main\nimplicit none\nprint *,{expression}\nend program\n",encoding="utf-8")
                for command in ([COMPILER,"run",source],[COMPILER,source,"-o",directory / "invalid.exe"]):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=60)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(diagnostic,result.stderr)

    def test_invalid_array_argument_types(self):
        for argument,diagnostic in [("a(.true.)","subscript must be INTEGER"),
                                    ("a(missing)","inquiry subscript type"),
                                    ("x(1)","wrong number of subscripts"),
                                    ("a(1,2)","wrong number of subscripts")]:
            with self.subTest(argument=argument),tempfile.TemporaryDirectory(prefix="numeric-subscript-") as tmp:
                directory=Path(tmp)
                source=directory / "invalid.f90"
                source.write_text(f"program main\nimplicit none\nreal::a(3),x\nprint *,digits({argument})\nend program\n",encoding="utf-8")
                result=subprocess.run([COMPILER,source,"-o",directory / "invalid.exe"],capture_output=True,text=True,timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(diagnostic,result.stderr)

    def test_intrinsic_names_remain_available_for_arrays_and_procedures(self):
        self.compare("""program main
implicit none
real::precision(2,2)
precision=7.0
print *,precision(1,2),digits(5)
contains
integer function digits(n)
integer,intent(in)::n
digits=n+100
end function digits
end program
""","7 105\n")

    def test_large_real_output_does_not_trap(self):
        self.compare("""program main
implicit none
real::x,a(1)
double precision::y
x=huge(0.0)
a(1)=x
y=huge(0.0_8)
print *,huge(0.0)
print *,x,a(1),x*1.0,-x
print *,y,huge(0.0_8),y*1.0_8,-y
write (*,*) x,y
print *,big()
contains
function big() result(value)
double precision::value
value=huge(0.0_8)
end function big
end program
""","3.4028234663852886e38\n"+
             "3.4028234663852886e38 3.4028234663852886e38 3.4028234663852886e38 -3.4028234663852886e38\n"+
             "1.7976931348623157e308 1.7976931348623157e308 1.7976931348623157e308 -1.7976931348623157e308\n"+
             "3.4028234663852886e38 1.7976931348623157e308\n1.7976931348623157e308\n")

    def test_real_at_integer_output_boundary(self):
        self.compare("""program main
implicit none
double precision::x,y
x=2.0_8**63
y=x*2.0_8
print *,x,-x,y
end program
""","9.223372036854776e18 -9.223372036854776e18 1.8446744073709552e19\n",
                     forbidden=("9223372036854775807.0",))


if __name__=="__main__":
    unittest.main(verbosity=2)
