"""Scalar CHARACTER relations, padding, and loop-condition reevaluation."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent


class CharacterComparisons(unittest.TestCase):
    def compare(self, source, expected, directory):
        for command in build_commands(source, directory):
            with self.subTest(command=command):
                result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assertTrue(equivalent(result.stdout, expected), result.stdout)
        if shutil.which("gfortran"):
            exe = directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(equivalent(result.stdout, expected), result.stdout)

    def test_relations(self):
        source = ROOT / "tests/114_character_comparisons.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="character-relations-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_loop_cycles_and_arguments(self):
        program = """program main
character(4)::a,b
integer::i,values(2)
a='a'
b='b'
i=0
do while (a < b)
  i=i+1
  a='c'
  cycle
end do
print *,i
do while (a < b)
  i=99
end do
print *,i
values=[1,2]
do while (sum(values) < 5)
  values=values+1
  cycle
end do
print *,values
call check('a','a  ')
call check('b','a')
contains
subroutine check(x,y)
character(*),intent(in)::x,y
print *,x.eq.y,x.ne.y,x.lt.y,x.le.y,x.gt.y,x.ge.y
end subroutine check
end program
"""
        with tempfile.TemporaryDirectory(prefix="character-loops-") as tmp:
            directory = Path(tmp)
            source = directory / "loops.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "1\n1\n2 3\nT F F T F T\nF T F F T T\n", directory)

    def test_invalid_operand(self):
        with tempfile.TemporaryDirectory(prefix="character-invalid-") as tmp:
            directory = Path(tmp)
            source = directory / "invalid.f90"
            source.write_text("program main\ncharacter(2)::a\na='a'\nprint *,a==1\nend\n", encoding="utf-8")
            result = subprocess.run([COMPILER, "emit-obj", source, directory / "invalid.obj"],
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("CHARACTER comparison requires two scalar CHARACTER operands", result.stderr)

    def test_function_arguments(self):
        program = """program main
character(5)::text
text='abcde'
print *,match3('abc'),match3('xyz'),match3(text),match3('a'//'bc')
print *,forward('abc'),forward('abz'),empty(''),empty('abc')
contains
integer function match3(x)
character(3)::x
match3=0
if(x=='abc') match3=3
end function match3
integer function forward(x)
character(3),intent(in)::x
forward=match3(x)
end function forward
integer function empty(x)
character(0),intent(in)::x
empty=0
if(x==' ') empty=1
end function empty
end program
"""
        with tempfile.TemporaryDirectory(prefix="character-functions-") as tmp:
            directory = Path(tmp)
            source = directory / "functions.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "3 0 3 3\n3 0 1 1\n", directory)

    def test_function_argument_diagnostics(self):
        cases = [
            ("'ab'", "", "shorter than its dummy"),
            ("'abc'", "x='xyz'", "must be read-only"),
        ]
        with tempfile.TemporaryDirectory(prefix="character-function-errors-") as tmp:
            directory = Path(tmp)
            for actual, statement, message in cases:
                with self.subTest(message=message):
                    source = directory / "invalid.f90"
                    source.write_text(f"program main\nprint *,f({actual})\ncontains\ninteger function f(x)\ncharacter(3)::x\n{statement}\nf=0\nif(x=='abc') f=1\nend function f\nend program\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "invalid.obj"],
                                            capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
