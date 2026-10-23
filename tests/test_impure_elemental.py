"""Impure elemental procedures: ordering, array arguments, and purity checks.

Run after cargo build --bin rfort: python tests/test_impure_elemental.py
RFORT_TEST_COMPILER selects another compiler build.
"""
import json
import math
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent


class ImpureElemental(unittest.TestCase):
    def compare(self, program, expected):
        with tempfile.TemporaryDirectory(prefix="impure-elemental-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program, encoding="utf-8")
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    left, right = result.stdout.split(), expected.split()
                    self.assertEqual(len(left), len(right), result.stdout)
                    for actual, wanted in zip(left, right):
                        if equivalent(actual, wanted):
                            continue
                        # Native list-directed output uses six significant digits.
                        self.assertTrue("." in wanted or "e" in wanted.lower(), result.stdout)
                        self.assertTrue(math.isclose(float(actual), float(wanted), rel_tol=5e-6), result.stdout)

    def test_arrays_sections_functions_optional_and_empty_calls(self):
        source = ROOT / "tests/126_impure_elemental.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        self.compare(source.read_text(encoding="utf-8"), expected)

    def test_side_effect_order_and_scalar_input_capture(self):
        self.compare("""program main
implicit none
integer::count,a(3),i,empty(0)
count=0
a=[3,1,2]
do i=1,2
call observe(a(3:1:-1)+i,10+i)
end do
call observe(empty,0)
empty=traced(empty)
print *,count
contains
impure elemental subroutine observe(value,offset)
integer,intent(in)::value,offset
count=count+1
print *,count,value,offset
end subroutine observe
elemental impure integer function traced(value)
integer,intent(in)::value
print *,999
traced=value
end function traced
end program main
""", "1 3 11\n2 2 11\n3 4 11\n4 4 12\n5 3 12\n6 5 12\n6\n")

    def test_declaration_and_call_diagnostics(self):
        routine = """impure elemental subroutine modify(x,y)
integer,intent(out)::x
integer,intent(in)::y
x=y
end subroutine modify
"""
        cases = [
            ("integer::a(2),b(3)", "call modify(a,b)", routine, "shapes do not conform"),
            ("integer::a,b(2)", "call modify(a,b)", routine, "requires an array actual"),
            ("", "call modify([1,2],[3,4])", routine, "section variable"),
            ("integer,parameter::a(2)=[1,2]", "call modify(a,3)", routine, "PARAMETER"),
            ("real::a(2)", "call modify(a,3)", routine, "type mismatch"),
            ("integer::a(2,2)", "call modify(a,3)", routine, "one-dimensional arrays"),
            ("integer::a(2)", "call modify(a)", routine, "missing required"),
            ("integer::a(2)", "call modify(a,3)", routine.replace("integer,intent(out)::x", "integer,intent(out)::x(2)"), "scalar dummy"),
            ("integer::a(2)", "call modify(a,3)", routine.replace("integer,intent(in)::y", "integer::y"), "explicit INTENT"),
            ("integer::a(2),b(2)", "call modify(a,b)", routine, "uninitialized variable `b`"),
        ]
        with tempfile.TemporaryDirectory(prefix="invalid-elemental-") as tmp:
            source = Path(tmp) / "main.f90"
            for declarations, statement, definition, message in cases:
                with self.subTest(message=message):
                    source.write_text(f"program main\n{declarations}\n{statement}\ncontains\n{definition}end program main\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "check", source], capture_output=True,
                                            text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)

    def test_supported_array_element_types(self):
        for kind in (1, 2, 4, 8):
            with self.subTest(integer_kind=kind):
                self.compare(f"""program main
implicit none
integer({kind})::a(2)
call fill(a,[1_{kind},2_{kind}])
print *,a
contains
impure elemental subroutine fill(x,y)
integer({kind}),intent(out)::x
integer({kind}),intent(in)::y
x=y+1_{kind}
end subroutine fill
end program main
""", "2 3\n")
        for kind in (4, 8):
            with self.subTest(real_kind=kind):
                self.compare(f"""program main
implicit none
real({kind})::a(2)
call fill(a,[1.5_{kind},2.5_{kind}])
print *,a
contains
impure elemental subroutine fill(x,y)
real({kind}),intent(out)::x
real({kind}),intent(in)::y
x=y+1.0_{kind}
end subroutine fill
end program main
""", "2.5 3.5\n")
        self.compare("""program main
implicit none
logical::a(2)
call fill(a,[.true.,.false.])
print *,a
contains
impure elemental subroutine fill(x,y)
logical,intent(out)::x
logical,intent(in)::y
x=.not.y
end subroutine fill
end program main
""", "F T\n")

    def test_pure_and_forall_contexts(self):
        for source, message in [
            ("elemental subroutine f(i)\ninteger,intent(in)::i\nprint *,i\nend subroutine f", "external I/O"),
            ("pure impure elemental subroutine f(i)\ninteger,intent(in)::i\nend subroutine f", "both PURE and IMPURE"),
            ("pure subroutine f(i)\ninteger,intent(in)::i\ncall g(i)\nend subroutine f\nimpure elemental subroutine g(i)\ninteger,intent(in)::i\nprint *,i\nend subroutine g", "impure or unknown subroutine"),
            ("pure integer function f(i)\ninteger,intent(in)::i\nf=g(i)\nend function f\nimpure elemental integer function g(i)\ninteger,intent(in)::i\ng=i\nend function g", "impure or unknown procedure"),
            ("integer::a(2),i\nforall(i=1:2) a(i)=f(i)\ncontains\nimpure elemental integer function f(i)\ninteger,intent(in)::i\nf=i\nend function f\nend", "FORALL cannot call impure"),
        ]:
            # Keep an explicit interface so these cases reach purity checking,
            # rather than the earlier ELEMENTAL interface-requirement error.
            if message in ("impure or unknown subroutine", "impure or unknown procedure"):
                source = "module purity_context\ncontains\n" + source + "\nend module purity_context\n"
            with self.subTest(message=message), tempfile.TemporaryDirectory(prefix="pure-context-") as tmp:
                path = Path(tmp) / "main.f90"
                path.write_text(source, encoding="utf-8")
                result = subprocess.run([COMPILER, "check", path], capture_output=True,
                                        text=True, timeout=15)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
