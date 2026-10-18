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
from compiler_frontend import Node
from rust_style import Plan
import ast


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
        for source in ("real :: x(2,2)", "character :: x", "call random_number(x)", "read *, x", "contains\ninteger function f()\nf=1\nend function f", "use m"):
            with self.subTest(source=source):
                self.reject(source, "unsupported|scalar|rank-one|undeclared")

    def test_uninitialized_reads_fail_with_source_line(self):
        with self.assertRaisesRegex(Unsupported, "before definite assignment") as caught:
            transpile(program("integer :: n\nprint *, n"))
        self.assertEqual(caught.exception.line, 4)

    def test_array_initialization_and_subset_limits(self):
        for body in ("real :: x(2)\nprint *, sum(x)",):
            self.reject(body,"definite assignment")
        self.reject("real :: x(3),y(2)\nx=1.0\ny=x","equal, statically known extent")
        self.reject("real :: x(3)\nx=1.0\nprint *, sum(x(1:3:2))","unit-stride")
        self.reject("real,parameter :: x=1.0\ncall random_number(x)","cannot assign PARAMETER")

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
        with self.assertRaisesRegex(Unsupported, "unsupported Rust backend expression: NewExpression"):
            self.frontend().expr({"NewExpression": {}}, 2)

    def test_unsupported_call_reports_its_name(self):
        with self.assertRaisesRegex(Unsupported, "unsupported Rust backend call: missing"):
            self.frontend().statements([{"CallSub":{"name":"missing","args":[]}}])

    def test_formatted_print_requires_literal_format(self):
        with self.assertRaisesRegex(Unsupported, "requires a literal format"):
            self.frontend().statements([{"CallSub":{"name":"@formatted_print","args":[{"Ident":"fmt"}]}}])

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

    def test_range_loop_requires_dead_post_loop_index(self):
        loop = Node("do",1,("i",[ast.Constant(1),ast.Constant(3)]))
        self.assertIn(id(loop),Plan([loop]).range_loops)
        observed = Node("print",2,[ast.Name(id="_rf_name_i")])
        self.assertNotIn(id(loop),Plan([loop,observed]).range_loops)

    def test_range_loop_does_not_hide_final_index_overflow(self):
        loop = Node("do",1,("i",[ast.Constant(2147483647),ast.Constant(2147483647)]))
        self.assertNotIn(id(loop),Plan([loop]).range_loops)

    def test_loop_carried_observations_prevent_inner_range_rewrite(self):
        inner = Node("do",2,("j",[ast.Constant(1),ast.Constant(3)]))
        outer = Node("do",1,("i",[ast.Constant(1),ast.Constant(3)]),[
            Node("print",2,[ast.Name(id="_rf_name_j")]),inner])
        self.assertNotIn(id(inner),Plan([outer]).range_loops)


@unittest.skipUnless(HAS_FRONTEND, "An existing rfort executable is required")
class StyleTests(unittest.TestCase):
    def test_bounded_example_is_compact_rust(self):
        code = transpile((ROOT / "examples" / "formatted.f90").read_text())
        self.assertIn("const N: i32 = 5;",code)
        self.assertIn("for i in 1..=N",code)
        self.assertIn('println!("{i:5}{:5}{:10.6}", (i * i), (i as f32).sqrt());',code)
        self.assertNotIn("let mut i",code)
        self.assertNotIn("rf_format",code)
        self.assertNotIn("rf_ipow",code)
        self.assertNotIn("rf_to_i32",code)

    def test_post_loop_variable_keeps_explicit_loop(self):
        code = transpile(program("integer :: i\ndo i=1,3\nprint*,i\nenddo\nprint*,i"))
        self.assertNotIn("for i in",code)
        self.assertIn("while if",code)

    def test_narrow_or_unknown_fields_keep_shared_formatter(self):
        for body in ('print "(i1)", 123', 'real :: x\nx=12345.0\nprint "(f5.1)",x',
                     'print "(f4.0)", 1.0', 'print "(i3.2)", 1'):
            with self.subTest(body=body):
                code = transpile(program(body))
                self.assertIn("mod rf_format",code)

    def test_non_const_intrinsic_parameter_uses_let(self):
        code = transpile(program("real,parameter::x=sqrt(2.0)\nprint*,x"))
        self.assertIn("let x: f32",code)
        self.assertNotIn("const X",code)


@unittest.skipUnless(HAS_FRONTEND and shutil.which("rustc") and shutil.which("gfortran"), "rfort, rustc, and gfortran required")
class NativeTests(unittest.TestCase):
    def test_array_print_and_element_initialization(self):
        self.compare(program("""integer :: a(3),i
real :: x(2)
double precision :: y(2)
a(1)=2
a(2)=3
a(3)=4
x(1)=1.25
x(2)=2.5
y(1)=3.0d0
y(2)=4.0d0
print *, 'mixed', a, 9, x, y
print *,a(2:),x(:1),y(2:1),a**2
print *,a,a
do i=1,3
   a(i)=i*10
end do
print *,a
"""))

    def test_partial_array_reads_and_runtime_initialization(self):
        self.compare(program("""real :: x(3)
integer :: i
x(2)=20.0
print *,x(2),x(2:2)
do i=1,3
   x(i)=real(i)
end do
print *,x,sum(x)
"""))
        self.compare(program("""integer :: a(2)
logical :: yes
yes=.true.
if (yes) then
   a(1)=1
else
   a(1)=2
end if
a(2)=3
print *,a
"""))

    def test_uninitialized_array_runtime_errors_identify_element_and_line(self):
        for read in ("x", "x(2)", "x(2:3)", "sum(x)"):
            with self.subTest(read=read):
                source = program(f"real :: x(3)\ninteger :: i\ndo i=1,1\nx(i)=10.0\nend do\nprint *,{read}")
                result = self.run_rust(source)
                self.assertNotEqual(result.returncode,0)
                self.assertIn("Fortran line 8",result.stderr)
                self.assertIn("uninitialized array `x`: element 2",result.stderr)

    def test_partial_branch_assignment_does_not_initialize_other_path(self):
        source = program("real :: x(2)\nlogical :: yes\nyes=.false.\nif (yes) then\nx(1)=10.0\nend if\nx(2)=20.0\nprint *,x")
        result = self.run_rust(source)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("uninitialized array `x`: element 1",result.stderr)

    def test_empty_array_print_items(self):
        self.compare(program("integer :: a(0),b(2)\nb=5\nprint *,a\nprint *,'empty',a,7,b(2:1)"))

    def test_existing_array_examples(self):
        for name in ("xdecl.f90","xsection.f90","xset_array.f90"):
            with self.subTest(name=name):
                self.compare((ROOT.parent / name).read_text())

    def run_rust(self, source):
        with tempfile.TemporaryDirectory(prefix="rust-array-test-",dir=ROOT.parent / "target") as directory:
            work = Path(directory)
            rust = work / "main.rs"
            exe = work / ("main.exe" if sys.platform=="win32" else "main.bin")
            rust.write_text(transpile(source),encoding="utf-8")
            result = subprocess.run(["rustc","--edition=2021","-D","warnings",str(rust),"-o",str(exe)],capture_output=True,text=True,timeout=120)
            self.assertEqual(result.returncode,0,result.stderr+rust.read_text())
            return subprocess.run([str(exe)],capture_output=True,text=True,timeout=10)

    def test_numeric_arrays_reductions_and_sections(self):
        self.compare(program("""integer,parameter :: n=4
integer :: a(n),b(n),i
real :: x(n)
double precision :: y(n)
a=2
a(2)=3
a(4)=-1
b=a+1
x=real(a)/2
y=dble(a)
print *,sum(a),product(a),minval(a),maxval(a),size(a)
print *,sum(a*a),sum(2*a),sum(a+2),sum(a**2),sum(2**a)
print *,sum(b),sum(x**2),product(y),minval(y),maxval(x)
print *,product(a(1:2)),sum(a(:)),sum(a(3:)),sum(a(:2))
a=a+1
do i=1,n
   a(i)=a(i)*i
end do
print *,sum(a),a(1),a(4)
"""))

    def test_empty_array_reduction_identities(self):
        self.compare(program("""integer :: a(0)
real :: x(0)
double precision :: y(0)
print *,sum(a),product(a),minval(a),maxval(a),size(a)
print *,sum(x),product(x),minval(x),maxval(x)
print *,sum(y),product(y),minval(y),maxval(y)
"""))

    def test_array_bounds_errors_include_fortran_line(self):
        for expression in ("x(0)","x(3)"):
            with self.subTest(expression=expression):
                result = self.run_rust(program(f"real :: x(2)\nx=1.0\nprint *,{expression}"))
                self.assertNotEqual(result.returncode,0)
                self.assertIn("Fortran line 5",result.stderr)
                self.assertIn("outside 1..=2",result.stderr)
        result = self.run_rust(program("real :: x(2)\ninteger :: last\nx=1.0\nlast=3\nprint *,sum(x(1:last))"))
        self.assertNotEqual(result.returncode,0)
        self.assertIn("Fortran line 7",result.stderr)
        self.assertIn("outside 1..=2",result.stderr)

    def test_random_array_example_and_shared_stream(self):
        source = (ROOT / "examples" / "random_array.f90").read_text()
        result = self.run_rust(source)
        self.assertEqual(result.returncode,0,result.stderr)
        values = [float(item) for item in result.stdout.split()]
        self.assertEqual(values[0],3)
        self.assertTrue(0.45 < values[1] < 0.55)
        self.assertTrue(0.28 < values[2] < 0.38)
        self.assertTrue(0 <= values[3] <= values[4] < 1)
        self.assertEqual(values[5],values[6])
        result = self.run_rust(program("""real :: x(3),s
double precision :: d
call random_number(x)
call random_number(s)
call random_number(d)
print *,x(1),x(2),x(3),s,d
"""))
        self.assertEqual(result.returncode,0,result.stderr)
        state,expected = 1,[]
        for precision in (24,24,24,24,53):
            state ^= state >> 12
            state ^= (state << 25) & (2**64-1)
            state ^= state >> 27
            bits = ((state*2685821657736338717) & (2**64-1)) >> (64-precision)
            expected.append(bits / 2**precision)
        actual_values = list(map(float,result.stdout.split()))
        self.assertEqual(len(actual_values),len(expected))
        for actual,want in zip(actual_values,expected):
            self.assertTrue(0 <= actual < 1)
            self.assertAlmostEqual(actual,want,places=7)

    def compare(self, source, *, tolerance=2e-6, exact=False):
        parent = ROOT.parent / "target"
        parent.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="rust-transpiler-test-", dir=parent) as directory:
            work = Path(directory)
            rust = work / "main.rs"
            fort = work / "main.f90"
            rust.write_text(transpile(source), encoding="utf-8")
            fort.write_text(source, encoding="utf-8")
            outputs = []
            raw_outputs = []
            for compiler, path, name, flags in (("rustc",rust,"rust",["--edition=2021","-D","warnings"]),
                                                 ("gfortran",fort,"fortran",["-std=f2018","-fcheck=all"])):
                exe = work / (name + (".exe" if sys.platform == "win32" else ".bin"))
                result = subprocess.run([compiler,*flags,str(path),"-o",str(exe)], cwd=work, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr + rust.read_text())
                result = subprocess.run([str(exe)], cwd=work, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                raw_outputs.append(result.stdout)
                outputs.append(result.stdout.split())
            if exact:
                self.assertEqual(raw_outputs[0], raw_outputs[1])
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

    def test_formatted_loop_matches_gfortran_exactly(self):
        self.compare((ROOT / "examples" / "formatted.f90").read_text(), exact=True)

    def test_formatted_scalar_types_and_nested_groups(self):
        self.compare(program('print "(a,1x,i5.3,1x,f8.2,1x,l3)", "value", -7, 1.25d0, .true.\nprint "(2(i3,1x),a)", 1, 2, "end"'), exact=True)

    def test_format_reversion_and_overflow_fields(self):
        self.compare(program('print "(i2)", 1, 2, 3\nprint "(i2,f5.1)", 1234, 12345.0\nprint "(\'literal only\')"'), exact=True)

    def test_optimized_loop_captures_bounds_before_body_changes(self):
        self.compare(program('integer,parameter::n=3\ninteger::i,first\nfirst=1\ndo i=first,n\nprint "(i3)",i\nfirst=99\nenddo'),exact=True)

    def test_literal_range_has_an_unambiguous_integer_type(self):
        self.compare(program('integer::i\ndo i=1,3\nprint*,abs(i)\nenddo'))

    def test_range_loop_and_fallback_preserve_empty_loop_index(self):
        self.compare(program('integer::i\ndo i=5,1\nprint*,99\nenddo\nprint "(i3)",i'),exact=True)

    def test_rust_keywords_and_generated_helper_name_collisions(self):
        self.compare(program('integer::loop,v_loop,rf_ipow\nloop=3\nv_loop=4\nrf_ipow=5\nprint "(3i5)",loop,v_loop,rf_ipow'),exact=True)

    def test_plain_formatting_for_zero_width_integer_and_negative_zero(self):
        self.compare(program('print "(i0)",-123\nprint "(f9.2)",-0.0'),exact=True)

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
