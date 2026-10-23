"""Keyword association in procedures, generics and OPTIONAL specialization."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_character_comparisons
from test_record_read import ROOT, COMPILER


class NamedArguments(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def check_program(self, program, expected):
        with tempfile.TemporaryDirectory(prefix="named-arguments-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, expected, directory)

    def test_procedures_and_optional_holes(self):
        source = ROOT / "tests/118_named_arguments.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="named-fixture-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_generic_selection_uses_associated_names_and_types(self):
        self.check_program("""module g
implicit none
interface f
module procedure f_ir,f_ri
end interface f
interface set_value
module procedure set_int,set_real
end interface set_value
interface optional_value
module procedure optional_specific
end interface optional_value
contains
pure integer function f_ir(first,second)
integer,intent(in)::first
real,intent(in)::second
f_ir=10+first+int(second)
end function f_ir
pure integer function f_ri(first,second)
real,intent(in)::first
integer,intent(in)::second
f_ri=20+int(first)+second
end function f_ri
subroutine set_int(dest,value)
integer,intent(out)::dest
integer,intent(in)::value
dest=value+1
end subroutine set_int
subroutine set_real(dest,value)
real,intent(out)::dest
real,intent(in)::value
dest=value+2
end subroutine set_real
pure integer function optional_specific(a,b,c)
integer,intent(in)::a,c
integer,intent(in),optional::b
optional_specific=a+c
if(present(b))optional_specific=optional_specific+b
end function optional_specific
end module g
program main
use g
implicit none
integer::i
real::x
print *,f(second=2.0,first=1),f(second=2,first=1.0)
call set_value(value=4,dest=i)
call set_value(value=4.0,dest=x)
print *,i,x
print *,optional_value(c=3,a=1),optional_value(c=3,b=2,a=1)
end program main
""", "13 23\n5 6\n4 6\n")

    def test_optional_forwarding_and_pure_output_calls(self):
        self.check_program("""program main
implicit none
integer::i
call outer(c=i,a=4)
print *,i
call outer(b=2,c=i,a=4)
print *,i
call driver(c=i)
print *,i
contains
pure subroutine outer(a,b,c)
integer,intent(in)::a
integer,intent(in),optional::b
integer,intent(out)::c
call inner(c=c,b=b,a=a)
end subroutine outer
pure subroutine inner(a,b,c)
integer,intent(in)::a
integer,intent(in),optional::b
integer,intent(out)::c
c=a
if(present(b))c=c+b
end subroutine inner
pure subroutine driver(c)
integer,intent(out)::c
call out_hole(c=c,a=7)
end subroutine driver
pure subroutine out_hole(a,b,c)
integer,intent(in)::a
integer,intent(out),optional::b
integer,intent(out)::c
c=a
if(present(b))b=a+1
end subroutine out_hole
end program main
""", "4\n6\n7\n")

    def test_named_array_and_character_actuals(self):
        self.check_program("""program main
implicit none
integer::values(3)
character(4)::text
values=[1,2,3]
call add(amount=5,n=3,a=values)
call fill(text='ok',dest=text)
print *,values,'[',text,']'
contains
subroutine add(a,n,amount)
integer,intent(in)::n,amount
integer,intent(inout)::a(:)
if(size(a)/=n)error stop 'shape mismatch'
a=a+amount
end subroutine add
subroutine fill(dest,text)
character(*),intent(out)::dest
character(*),intent(in)::text
dest=text
end subroutine fill
end program main
""", "6 7 8 [ok  ]\n")

    def test_named_intrinsics_and_random_harvest(self):
        self.check_program("""program main
implicit none
real::x(3)
print *,sqrt(x=4.0),mod(p=3,a=8),abs(a=-2)
print *,atan2(x=1.0,y=0.0),real(kind=8,a=2),int(a=3.5)
print *,sum(mask=[.true.,.false.],array=[2,3])
call random_number(harvest=x)
print *,all(x>=0.0 .and. x<1.0)
end program main
""", "2 2 2\n0 2 3\n2\n.TRUE.\n")

    def test_named_derived_and_bound_procedures(self):
        self.check_program("""module boxes
implicit none
type::box
integer::value
contains
procedure::add_value
end type box
contains
subroutine add_value(self,amount)
class(box),intent(inout)::self
integer,intent(in)::amount
self%value=self%value+amount
end subroutine add_value
pure function make_box(value) result(b)
integer,intent(in)::value
type(box)::b
b=box(value)
end function make_box
end module boxes
program main
use boxes
implicit none
type(box)::b
b=make_box(value=5)
call b%add_value(amount=3)
print *,b%value
end program main
""", "8\n")

    def test_invalid_generic_keywords(self):
        program = """module g
implicit none
interface pick
module procedure pick_int,pick_real
end interface pick
contains
integer function pick_int(x,y)
integer,intent(in)::x,y
pick_int=x+y
end function pick_int
real function pick_real(x,y)
real,intent(in)::x,y
pick_real=x+y
end function pick_real
end module g
program main
use g
implicit none
print *,CALL
end program main
"""
        with tempfile.TemporaryDirectory(prefix="named-generic-errors-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            for call, message in [
                ("pick(z=1)", "unknown keyword `z`"),
                ("pick(x=1,x=2,y=3)", "duplicate argument `x`"),
                ("pick(x=1,2)", "positional argument after keyword"),
                ("pick(x=1,y=2.0)", "no matching specific procedure"),
            ]:
                with self.subTest(call=call):
                    source.write_text(program.replace("CALL", call), encoding="utf-8")
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "test.obj"], capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)

    def test_invalid_calls(self):
        routine = "subroutine s(a,b)\ninteger,intent(in)::a,b\nprint *,a,b\nend subroutine s"
        function = "integer function f(a,b)\ninteger,intent(in)::a,b\nf=a+b\nend function f"
        cases = [
            ("call s(a=1,z=2)", routine, "unknown keyword `z`"),
            ("call s(a=1,a=2,b=3)", routine, "duplicate argument `a`"),
            ("call s(1,a=2)", routine, "duplicate argument `a`"),
            ("call s(a=1,2)", routine, "positional argument after keyword"),
            ("call s(b=2)", routine, "missing required argument `a`"),
            ("call s(1,2,3,b=4)", routine, "too many arguments"),
            ("print *,f(b=2,a=1,a=3)", function, "duplicate argument `a`"),
            ("print *,f(a=1)", function, "missing required argument `b`"),
            ("call missing(x=1)", "", "requires an explicit interface"),
            ("print *,sqrt(y=1.0)", "", "unknown keyword `y`"),
            ("print *,array(i=1)", "", "keyword arguments cannot be used in array subscripts"),
            ("call s(dest=1)", "subroutine s(dest)\ninteger,intent(out)::dest\ndest=1\nend subroutine s", "writable argument"),
        ]
        with tempfile.TemporaryDirectory(prefix="named-errors-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            for statement, procedure, message in cases:
                with self.subTest(statement=statement):
                    source.write_text(f"program main\ninteger::array(2)\n{statement}\ncontains\n{procedure}\nend program main\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "test.obj"], capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
