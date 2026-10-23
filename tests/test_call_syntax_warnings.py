"""Interfaces, intrinsic names, underflow, CASE ranges, and statement labels."""
import unittest
from pathlib import Path
import subprocess
import tempfile
from test_record_read import COMPILER
import test_unused_warnings as shared


class CallSyntaxWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_implicit_external_interface(self):
        source = """program main
implicit none
call work(3)
print *,twice(4)
contains
integer function twice(i)
integer,intent(in)::i
twice=2*i
end function
end program
subroutine work(i)
integer,intent(in)::i
print *,i
end subroutine
"""
        self.assertEqual(self.check(source), "")
        warning = self.check(source, ["-Wimplicit-interface"])
        self.assertEqual(warning.count("[implicit-interface]"), 1, warning)
        self.assertIn("call to `work`", warning)
        self.assertIn("example.f90:3:", warning)
        self.assertNotIn("[implicit-interface]", self.check(source, ["-Wall"]))
        self.assertEqual(self.check(source, ["-Wimplicit-interface", "-Wno-implicit-interface"]), "")
        self.assertIn("error:", self.check(source, ["-Werror=implicit-interface"], errors=True))

    def test_explicit_interface_block(self):
        source = """program main
interface
subroutine work(i)
integer,intent(in)::i
end subroutine
end interface
call work(3)
end program
subroutine work(i)
integer,intent(in)::i
print *,i
end subroutine
"""
        self.assertEqual(self.check(source, ["-Wimplicit-interface"]), "")

    def test_module_generic_and_intrinsics(self):
        source = """module m
interface twice
module procedure f
end interface
contains
real function f(x)
real,intent(in)::x
f=2*x
end function
end module
program main
use m
real::x(2)
x=1
print *,twice(2.0),sum(x),sqrt(4.0)
call random_number(x)
end program
"""
        self.assertEqual(self.check(source, ["-Wimplicit-interface"]), "")

    def test_keyword_requires_explicit_even_when_warning_disabled(self):
        source = """program main
call work(i=3)
end program
subroutine work(i)
integer,intent(in)::i
print *,i
end subroutine
"""
        warning = self.check(source, ["-Wno-implicit-interface"], errors=True)
        self.assertIn("requires an explicit interface", warning)
        self.assertIn("keyword arguments", warning)
        self.assertIn("example.f90:2:", warning)

    def test_optional_requires_explicit(self):
        source = """program main
call work(3)
end program
subroutine work(i)
integer,optional,intent(in)::i
if(present(i))print *,i
end subroutine
"""
        warning = self.check(source, errors=True)
        self.assertIn("requires an explicit interface", warning)
        self.assertIn("OPTIONAL", warning)

    def test_assumed_shape_requires_explicit(self):
        source = """program main
integer::a(2)
a=1
call work(a)
end program
subroutine work(a)
integer,intent(in)::a(:)
print *,a
end subroutine
"""
        warning = self.check(source, errors=True)
        self.assertIn("requires an explicit interface", warning)
        self.assertIn("assumed-shape", warning)

    def test_dummy_procedure_implicit_interface(self):
        source = """program main
call apply(work)
contains
subroutine work(i)
integer,intent(in)::i
print *,i
end subroutine
subroutine apply(f)
external::f
call f(3)
end subroutine
end program
"""
        warning = self.check(source, ["-Wimplicit-interface"])
        self.assertEqual(warning.count("[implicit-interface]"), 1, warning)
        self.assertIn("call to `f`", warning)

    def test_intrinsic_shadow_is_opt_in_and_not_data_name(self):
        source = """program main
real::sum(2)
sum=1
print *,sum,sqrt(4.0)
contains
real function sqrt(x)
real,intent(in)::x
sqrt=x
end function
end program
"""
        self.assertEqual(self.check(source), "")
        warning = self.check(source, ["-Wintrinsic-shadow"])
        self.assertEqual(warning.count("[intrinsic-shadow]"), 1, warning)
        self.assertIn("user procedure `sqrt`", warning)
        self.assertIn("example.f90:6:", warning)
        self.assertNotIn("user procedure `sum`", warning)
        self.assertNotIn("[intrinsic-shadow]", self.check(source, ["-Wall"]))

    def test_intrinsic_declaration_does_not_shadow(self):
        source = "intrinsic::sqrt\nprint *,sqrt(4.0)\nend\n"
        self.assertEqual(self.check(source, ["-Wintrinsic-shadow", "-Wimplicit-interface"]), "")

    def test_underflow_literal_and_conversion(self):
        source = "real::x\nx=1e-50\nprint *,x\nend\n"
        warning = self.check(source, ["-Wunderflow"])
        self.assertEqual(warning.count("[underflow]"), 1, warning)
        self.assertIn("rounds to zero in REAL(4)", warning)
        self.assertIn("example.f90:2:", warning)
        self.assertIn("[underflow]", self.check(source, ["-Wall"]))
        self.assertEqual(self.check(source, ["-Wunderflow", "-Wno-underflow"]), "")
        self.assertIn("error:", self.check(source, ["-Werror=underflow"], errors=True))

    def test_underflow_double_beyond_host_range(self):
        source = "double precision::x\nx=1d-400\nprint *,x\nend\n"
        warning = self.check(source, ["-Wunderflow"])
        self.assertEqual(warning.count("[underflow]"), 1, warning)
        self.assertIn("REAL(8)", warning)
        source = "double precision::x\nx=1d-300*1d-300\nprint *,x\nend\n"
        self.assertIn("[underflow]", self.check(source, ["-Wunderflow"]))

    def test_underflow_nonzero_subnormals_zero_and_cancellation(self):
        source = """implicit none
real::x
double precision::y
x=1e-40
x=0e-999
x=1.0-1.0
y=1d-320
y=4.9406564584124654d-324
y=1d-300-1d-300
print *,x,y
end
"""
        self.assertEqual(self.check(source, ["-Wunderflow"]), "")

    def test_underflow_type_inquiries_do_not_evaluate_argument(self):
        self.assertEqual(self.check("print *,kind(1e-50),tiny(1e-50)\nend\n", ["-Wunderflow"]), "")

    def test_underflow_scoped_kind_and_parameter(self):
        source = """use iso_fortran_env,only:real64
implicit none
real(real64),parameter::small=1e-100_real64
real::x
x=small
print *,x
end
"""
        warning = self.check(source, ["-Wunderflow"])
        self.assertEqual(warning.count("[underflow]"), 1, warning)
        self.assertIn("example.f90:5:", warning)

    def test_empty_integer_case_and_location(self):
        source = """integer::i
i=2
select case(i)
case(10:1)
print *,1
case default
print *,2
end select
end
"""
        warning = self.check(source, ["-Wempty-case"])
        self.assertEqual(warning.count("[empty-case]"), 1, warning)
        self.assertIn("range 10:1 is empty", warning)
        self.assertIn("example.f90:4:", warning)

    def test_character_case_blank_padding(self):
        source = """character::s
s='a'
select case(s)
case('z':'b')
print *,1
case default
print *,2
end select
end
"""
        warning = self.check(source, ["-Werror=empty-case"], errors=True)
        self.assertIn("[empty-case]", warning)
        # Native CHARACTER CASE ranges remain unsupported; ensure blank padding
        # produces no warning even though the backend rejects the construct.
        quiet = self.check(source.replace("'z':'b'", "'a ':'a'"), ["-Wempty-case"], errors=True)
        self.assertNotIn("[empty-case]", quiet)

    def test_empty_enumeration_case(self):
        source = """enumeration type::colour
enumerator::red,green,blue
end enumeration type
type(colour)::c
c=red
select case(c)
case(blue:red)
print *,1
case default
print *,2
end select
end
"""
        self.assertIn("[empty-case]", self.check(source, ["-Wempty-case"]))

    def test_unused_statement_and_format_labels(self):
        source = """print *,1
10 continue
20 format(i3)
end
"""
        warning = self.check(source, ["-Wunused-label"])
        self.assertEqual(warning.count("[unused-label]"), 2, warning)
        self.assertIn("label 10", warning)
        self.assertIn("label 20", warning)
        self.assertIn("example.f90:2:", warning)

    def test_referenced_goto_and_format_labels(self):
        source = "goto 0010\n10 print 20,1\n20 format(i3)\nend\n"
        self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_labeled_shared_do_terminal(self):
        source = """integer::i,j
do 100 i=1,2
do 100 j=1,2
print *,i,j
100 continue
end
"""
        self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_do_terminal_continuation(self):
        source = "integer::i\ndo & ! terminal below\n & 100 i=1,2\nprint *,i\n100 continue\nend\n"
        self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_labels_are_per_program_unit(self):
        source = """program main
goto 10
10 call work()
contains
subroutine work()
10 print *,1
end subroutine
end program
"""
        warning = self.check(source, ["-Wunused-label"])
        self.assertEqual(warning.count("[unused-label]"), 1, warning)
        self.assertIn("example.f90:6:", warning)

    def test_io_end_reference(self):
        source = """integer::x
read(*,*,end=20)x
print *,x
20 continue
end
"""
        self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_underflow_inquiry_result_and_expression(self):
        for expression in ("tiny(1d0)", "tiny(1.0)/huge(1.0)", "tiny(1d0)/huge(1d0)", "exp(-200.0)"):
            with self.subTest(expression=expression):
                source = f"real::x\nx={expression}\nprint *,x\nend\n"
                self.assertIn("[underflow]", self.check(source, ["-Wunderflow"]))

    def test_format_control_positions(self):
        for io in ("write(*,20) 1", "write(unit=6,fmt=20) 1", "write(fmt=20,unit=6) 1", "read(*,20) x", "read 20,x"):
            with self.subTest(io=io):
                source = f"integer::x\n{io}\n20 format(i3)\nend\n"
                self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_unit_number_is_not_format_reference(self):
        source = "write(20,*) 1\n20 format(i3)\nend\n"
        self.assertIn("label 20", self.check(source, ["-Wunused-label"]))

    def test_multifile_module_interface_and_call_location(self):
        with tempfile.TemporaryDirectory(prefix="call-warnings-") as tmp:
            root = Path(tmp)
            module = root / "m.f90"
            main = root / "main.f90"
            module.write_text("module m\ncontains\nsubroutine work(i)\ninteger,intent(in)::i\nprint *,i\nend subroutine\nend module\n", encoding="utf-8")
            main.write_text("use m\ncall work(i=3)\nend\n", encoding="utf-8")
            result = subprocess.run([COMPILER, "-Wimplicit-interface", module, main, "-o", root / "main.exe"], capture_output=True, text=True, timeout=60, cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertNotIn("[implicit-interface]", result.stderr)
            module.write_text("subroutine work(i)\ninteger,optional,intent(in)::i\nif(present(i))print *,i\nend subroutine\n", encoding="utf-8")
            main.write_text("call work(3)\nend\n", encoding="utf-8")
            result = subprocess.run([COMPILER, module, main, "-o", root / "main.exe"], capture_output=True, text=True, timeout=60, cwd=root)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("main.f90:1:", result.stderr)
            self.assertIn("m.f90:1:", result.stderr)
            self.assertIn("explicit procedure interface required", result.stderr)

    def test_external_intrinsic_name_still_needs_interface(self):
        source = "program main\nreal,external::sqrt\nprint *,sqrt(4.0)\nend program\nreal function sqrt(x)\nreal,intent(in)::x\nsqrt=x\nend function\n"
        warning = self.check(source, ["-Wimplicit-interface", "-Wintrinsic-shadow"])
        self.assertIn("call to `sqrt`", warning)
        self.assertIn("[intrinsic-shadow]", warning)

    def test_computed_goto_references(self):
        source = "integer::i\ni=1\ngoto(10,20),i\n10 print *,1\ngoto 30\n20 print *,2\n30 continue\nend\n"
        self.assertEqual(self.check(source, ["-Wunused-label"]), "")

    def test_controls_and_directives(self):
        source = "!#allow(unused-label)\n10 print *,1\nend\n"
        self.assertNotIn("[unused-label]", self.check(source, ["-Wall"]))
        self.assertIn("[unused-label]", self.check(source.replace("allow", "deny"), errors=True))


if __name__ == "__main__":
    unittest.main()
