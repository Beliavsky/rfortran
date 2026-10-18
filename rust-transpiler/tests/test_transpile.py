import math
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from transpile import Unsupported, transpile
from compiler_frontend import Frontend, FrontendError, find_compiler


try:
    find_compiler()
    HAS_FRONTEND = True
except FrontendError:
    HAS_FRONTEND = False


def program(body):
    return "program demo\nimplicit none\n" + body + "\nend program demo\n"


@unittest.skipUnless(HAS_FRONTEND, "An existing rfort executable is required")
class ValidationTests(unittest.TestCase):
    def reject(self, source, message):
        with self.assertRaisesRegex((Unsupported, FrontendError), message):
            transpile(program(source))

    def test_unknown_constructs_fail(self):
        for source in ("real :: x(2)", "character :: x", "call random_number(x)", "read *, x", "contains\ninteger function f()\nf=1\nend function f", "use m"):
            with self.subTest(source=source):
                self.reject(source, "unsupported|scalar")

    def test_uninitialized_reads_fail_with_source_line(self):
        with self.assertRaisesRegex(Unsupported, "before definite assignment") as caught:
            transpile(program("integer :: n\nprint *, n"))
        self.assertEqual(caught.exception.line, 4)

    def test_branch_and_zero_trip_loop_initialization(self):
        self.reject("integer :: n\nif (.true.) then\nn=1\nend if\nprint *, n", "definite assignment")
        self.reject("integer :: i,n\ndo i=2,1\nn=1\nend do\nprint *, n", "definite assignment")
        transpile(program("integer :: n\nif (.true.) then\nn=1\nelse\nn=2\nend if\nprint *, n"))

    def test_parameters_and_active_do_variables_cannot_be_assigned(self):
        self.reject("integer, parameter :: n=2\nn=3", "cannot assign PARAMETER")
        self.reject("integer :: i\ndo i=1,2\ni=4\nend do", "active DO")
        self.reject("integer :: x=2\ninteger, parameter :: n=x", "only named constants")

    def test_malformed_programs_and_late_declarations(self):
        self.reject("print *, 1\ninteger :: i", "declarations must precede")
        with self.assertRaisesRegex(Unsupported, "requires IMPLICIT NONE"):
            transpile("program p\nprint *, 1\nend\n")

    def test_compiler_syntax_errors_and_backend_limits_are_reported(self):
        for expression in ("0x10", "1_000", "[1,2]", "__import__('os')", "1 if .true. else 2", "2 // 1", "1 % 2"):
            with self.subTest(expression=expression):
                self.reject("print *, " + expression, "unsupported|invalid|not yet supported|rfort parse failed")

    def test_types_and_intrinsic_signatures_are_checked(self):
        for body in ("integer :: n\nn=.true.", "print *, sqrt(4)", "print *, max(1,2.0)",
                     "print *, real(1,8)", "print *, random_number(1)", "if (1) then\nend if"):
            with self.subTest(body=body):
                self.reject(body, "convert|unsupported|LOGICAL|same type|one positional")

    def test_continuations_strings_and_comments(self):
        generated = transpile(program('integer :: x\nx = 1 + &\n & 2 ! comment\nprint *, "don\'t ! split; this", x; print *, \'a\'\'b\''))
        self.assertIn("don't ! split; this", generated)
        self.assertIn("a'b", generated)


class AdapterTests(unittest.TestCase):
    def frontend(self, source="implicit none\nprint *, 1\nend\n"):
        frontend = object.__new__(Frontend)
        frontend.source, frontend.raw, frontend.filename = source, source.encode("utf-8"), "test.f90"
        return frontend

    def test_unknown_compiler_ast_is_not_silently_ignored(self):
        with self.assertRaisesRegex(Unsupported, "unsupported Rust backend statement: NewStatement"):
            self.frontend().statements([{"NewStatement": {}}])

    def test_unknown_compiler_expression_is_rejected(self):
        with self.assertRaisesRegex(Unsupported, "unsupported Rust backend expression: Section"):
            self.frontend().expr({"Section": {"name":"x"}}, 2)

    def test_compiler_source_offsets_are_utf8_bytes(self):
        frontend = self.frontend("! \u03bc\nprint *, 1\nend\n")
        self.assertEqual(frontend.line(len("! \u03bc\n".encode("utf-8"))), 2)

    def test_semantic_check_failure_is_not_bypassed(self):
        import json
        frontend = self.frontend()
        phases = []
        def command(args, directory, snapshot):
            phases.append(args[0])
            if args[0] == "parse":
                Path(args[2]).write_text(json.dumps({"name":"main", "body":[{"Implicit":"None"},{"Print":{"items":[{"IntLit":"1"}]}}]}))
            else:
                raise FrontendError("compiler semantic diagnostic")
        with mock.patch.object(frontend, "command", side_effect=command):
            with self.assertRaisesRegex(FrontendError, "compiler semantic diagnostic"):
                frontend.process(lambda body: "unused generated Rust")
        self.assertEqual(phases, ["parse", "check"])

    def test_missing_compiler_reports_error_without_parser_fallback(self):
        with self.assertRaisesRegex(FrontendError, "executable not found"):
            find_compiler("definitely_missing_rfort_executable_123456")


@unittest.skipUnless(HAS_FRONTEND and shutil.which("rustc") and shutil.which("gfortran"), "rfort, rustc, and gfortran required")
class NativeTests(unittest.TestCase):
    def compare(self, source, *, tolerance=2e-6):
        parent = ROOT.parent / "target"
        parent.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="rust-transpiler-test-", dir=parent) as directory:
            work = Path(directory)
            rust = work / "main.rs"
            fort = work / "main.f90"
            rust.write_text(transpile(source), encoding="utf-8")
            fort.write_text(source, encoding="utf-8")
            outputs = []
            for compiler, path, name, flags in (("rustc",rust,"rust",["--edition=2021","-D","warnings"]),
                                                 ("gfortran",fort,"fortran",["-std=f2018","-fcheck=all"])):
                exe = work / (name + (".exe" if sys.platform == "win32" else ".bin"))
                result = subprocess.run([compiler,*flags,str(path),"-o",str(exe)], cwd=work, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr + rust.read_text())
                result = subprocess.run([str(exe)], cwd=work, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                outputs.append(result.stdout.split())
            self.assertEqual(len(outputs[0]), len(outputs[1]), outputs)
            for actual, expected in zip(*outputs):
                try:
                    a = float(actual.replace("D","E").replace("d","e"))
                    b = float(expected.replace("D","E").replace("d","e"))
                except ValueError:
                    self.assertEqual(actual, expected)
                else:
                    self.assertTrue(math.isclose(a,b,rel_tol=tolerance,abs_tol=tolerance), (actual,expected))

    def test_scalar_example(self):
        self.compare((ROOT / "examples" / "scalars.f90").read_text())

    def test_integer_division_powers_and_precedence(self):
        self.compare(program("print *, -7/3, 7/(-3), 2**3**2, -2**2, (-2)**2\nprint *, 2**(-3), (-1)**(-3), 0**0, mod(-7,3)"))

    def test_negative_and_zero_trip_loops_and_saved_bounds(self):
        self.compare(program("""integer :: i, limit, stride, total
limit=5
stride=2
total=0
do i=1,limit,2
   limit=0
   stride=1
   total=total+i
end do
print *, total,i
do i=3,-2,-2
   print *, i
end do
print *, i
do i=5,1
   total=999
end do
print *, total,i"""))

    def test_compiler_grammar_declarations_without_colons(self):
        # The removed Python parser rejected these; rfort already supports them.
        self.compare("program p\nimplicit none\ninteger i\nreal x\ni=3\nx=sqrt(real(i))\nprint*,i,x\nend program p\n")

    def test_real_and_double_math(self):
        self.compare(program("""real :: x
double precision :: y
x=.5
y=1.d-2
print *, sqrt(x),exp(x),log(x),log10(x),sin(x),cos(x),tan(x)
print *, asin(x),acos(x),atan(x),sinh(x),cosh(x),tanh(x),abs(-x)
print *, int(-2.9), dble(3)/2.d0, y, 2**.5, 2.d0**(-2)
print *, min(x,1.,2.), max(y,3.d0,4.d0)"""))

    def test_logical_expressions_and_keyword_identifiers(self):
        self.compare(program("""logical :: and, or, not
integer :: i
and=.true.
or=.false.
not=and .and. .not.or
i=2
print *, not, i.eq.2, 2.ne.i, .not.(i>0).or..false.
if (not) then
   if (i==2) then
      print *, 10
   else
      print *, 11
   end if
else
   print *, 12
end if"""))

    def test_whitespace_and_empty_print(self):
        self.compare("program p\nimplicit    none\nif (.true.) then\nprint*\nend    if\nend program p\n")

    def test_existing_small_programs(self):
        for filename in ("xloop.f90", "xmath.f90"):
            path = ROOT.parent / filename
            if path.exists():
                with self.subTest(filename=filename):
                    self.compare(path.read_text())


if __name__ == "__main__":
    unittest.main()
