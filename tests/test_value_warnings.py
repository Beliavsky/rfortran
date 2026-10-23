"""Typed numeric advice, pointer defaults, loop bounds, and enum coverage."""
import unittest
from pathlib import Path
import subprocess
import tempfile
import test_unused_warnings as shared
from test_record_read import COMPILER


class ValueWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_conversion_integer_narrowing(self):
        source = """implicit none
integer(kind=8)::big
integer::small
big=123
small=big
print *,small
end
"""
        self.assertEqual(self.check(source), "")
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[conversion]"), 1, warning)
        self.assertIn("INTEGER(8) to INTEGER(4)", warning)
        self.assertIn("example.f90:5:", warning)
        self.assertNotIn("[conversion]", self.check(source, ["-Wall"]))
        self.assertEqual(self.check(source, ["-Wconversion", "-Wno-conversion"]), "")
        self.assertIn("[conversion]", self.check(source, ["-Werror=conversion"], errors=True))

    def test_conversion_exact_constants_and_widening(self):
        source = """use iso_fortran_env,only:int64,real64
implicit none
integer(int64)::big
integer::small
real(real64)::wide
real::single
small=12_int64
big=small
wide=small
single=0.5_real64
small=2.0_real64
print *,small,big,wide,single
end
"""
        self.assertEqual(self.check(source, ["-Wconversion"]), "")

    def test_conversion_real_and_complex(self):
        source = """implicit none
double precision::wide
real::single
integer::i
complex(kind=8)::z
wide=1.5d0
z=(1.0d0,2.0d0)
single=wide
i=wide
single=z
print *,single,i
end
"""
        warning = self.check(source, ["-Werror=conversion"], errors=True)
        self.assertEqual(warning.count("[conversion]"), 3, warning)
        self.assertIn("COMPLEX(8) to REAL(4)", warning)
        self.assertIn("REAL(8) to INTEGER(4)", warning)

    def test_conversion_integer_to_real_precision(self):
        source = """implicit none
integer::i
real::r
i=16777217
r=i
print *,r
end
"""
        self.assertIn("[conversion]", self.check(source, ["-Wconversion"]))
        self.assertEqual(self.check(source.replace("r=i", "r=16777216"), ["-Wconversion"]), "")
        self.assertIn("[conversion]", self.check(source.replace("r=i", "r=16777217"), ["-Wconversion"]))

    def test_conversion_explicit(self):
        source = """implicit none
double precision::wide
real::single
integer::i
wide=1.5d0
single=real(wide)
i=int(wide)
print *,single,i
end
"""
        self.assertEqual(self.check(source, ["-Wconversion"]), "")

    def test_explicit_complex_conversion_uses_default_kind(self):
        source = """complex(kind=8)::wide
complex::small
wide=(1.0d0,2.0d0)
small=cmplx(wide)
print *,small
end
"""
        self.assertEqual(self.check(source, ["-Wconversion"]), "")

    def test_kinds_from_typed_literal_inquiries(self):
        source = """use iso_fortran_env,only:real64
implicit none
integer,parameter::k8=kind(0_8),r8=kind(0.0_real64)
integer(k8)::big
integer::small
real(r8)::wide
real::single
big=1
small=big
wide=1.0d0
single=wide
print *,small,single
end
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[conversion]"), 2, warning)
        self.assertIn("INTEGER(8) to INTEGER(4)", warning)
        self.assertIn("REAL(8) to REAL(4)", warning)

    def test_invalid_extreme_dim_preserves_error_diagnostic(self):
        source = "integer::i,a(2)\na=0\ndo i=lbound(a,dim=-huge(1_8)-1),2\nprint *,a(i)\nend do\nend\n"
        warning = self.check(source, ["-Wdo-subscript"], errors=True)
        self.assertNotIn("panicked", warning)

    def test_conversion_constant_already_rounded_before_widening(self):
        source = "real::r\nr=dble(0.1)\nprint *,r\nend\n"
        self.assertEqual(self.check(source, ["-Wconversion", "-Wprecision-loss"]), "")

    def test_precision_calculation_and_literal(self):
        source = """use iso_fortran_env,only:real64
implicit none
real(real64)::x,y,z
x=sqrt(2.0)
y=0.1
z=1.0/3.0
print *,x,y,z
end
"""
        warning = self.check(source, ["-Wprecision-loss"])
        self.assertEqual(warning.count("[precision-loss]"), 3, warning)
        self.assertIn("REAL(4) precision before widening to REAL(8)", warning)
        self.assertIn("example.f90:4:", warning)
        self.assertNotIn("real_precision_strip", warning)
        self.assertIn("[precision-loss]", self.check(source, ["-Wall"]))

    def test_precision_exact_and_intentional_narrowing(self):
        source = """use iso_fortran_env,only:real64
implicit none
real(real64)::x
real::s
x=1.0
x=0.5
x=1.0+2.0
x=sqrt(4.0)
x=sqrt(2.0_real64)
x=1.0d0/3.0d0
s=0.123456789
x=real(1.0_real64/3.0_real64)
print *,x,s
end
"""
        self.assertEqual(self.check(source, ["-Wprecision-loss"]), "")

    def test_precision_widening_conversion_and_mixed_expression(self):
        source = """implicit none
double precision::x
x=dble(sqrt(2.0))
x=1.0d0+0.1
print *,x
end
"""
        warning = self.check(source, ["-Wprecision-loss"])
        self.assertEqual(warning.count("[precision-loss]"), 2, warning)

    def test_precision_scoped_named_kinds(self):
        source = """module kinds
integer,parameter::dp=selected_real_kind(15)
end module
program main
use kinds,only:wide=>dp
implicit none
real(wide)::x
x=sqrt(2.0)
print *,x
end program
"""
        self.assertIn("[precision-loss]", self.check(source, ["-Wprecision-loss"]))

    def test_precision_array_constants(self):
        source = """implicit none
double precision::a(2)
a=[1.0,2.0]
print *,a
end
"""
        self.assertEqual(self.check(source, ["-Wprecision-loss"]), "")
        warning = self.check(source.replace("[1.0,2.0]", "[0.1,0.2]"), ["-Wprecision-loss"])
        self.assertEqual(warning.count("[precision-loss]"), 1, warning)

    def test_conversion_components_and_function_results(self):
        source = """program main
implicit none
type::record
real::value
end type
type(record)::r
r%value=f()
print *,r%value
contains
double precision function f()
f=0.5d0
end function
end program
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[conversion]"), 1, warning)
        self.assertIn("assignment to `r%value`", warning)

    def test_conversion_component_array_element(self):
        source = """type::record
double precision::values(2)
end type
type(record)::r
real::x
r%values=[1.0d0,2.0d0]
x=r%values(1)
print *,x
end
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[conversion]"), 1, warning)

    def test_conversion_integer64_boundary(self):
        source = """use iso_fortran_env,only:int64,real64
implicit none
real(real64)::x
x=9223372036854775807_int64
print *,x
end
"""
        self.assertIn("[conversion]", self.check(source, ["-Wconversion"]))

    def test_loop_guard_does_not_evaluate_short_circuit(self):
        source = """integer::i,a(10)
a=0
do i=1,11
if(i<=10.and.a(i)>0)print *,i
end do
end
"""
        self.assertIn("[do-subscript]", self.check(source, ["-Wdo-subscript"]))

    def test_loop_stride_uses_last_reachable_value(self):
        source = "integer::i,a(9)\na=0\ndo i=1,10,2\nprint *,a(i)\nend do\nend\n"
        self.assertEqual(self.check(source, ["-Wdo-subscript"]), "")

    def test_loop_facts_invalidated_by_assignment(self):
        source = """integer::i,j,a(10)
a=0
do i=1,11
j=i
if(i<2)then
cycle
else
j=1
end if
print *,a(j)
end do
end
"""
        self.assertEqual(self.check(source, ["-Wdo-subscript"]), "")

    def test_pointer_component_defaults(self):
        source = """module m
type::item
real,pointer::missing,ready=>null()
integer,pointer::values(:)
real,allocatable::owned(:)
end type
end module
"""
        warning = self.check(source, ["-Wpointer-init"])
        self.assertEqual(warning.count("[pointer-init]"), 2, warning)
        self.assertIn("item%missing", warning)
        self.assertIn("item%values", warning)
        self.assertNotIn("item%ready", warning)
        self.assertNotIn("item%owned", warning)
        self.assertIn("example.f90:3:", warning)
        self.assertEqual(self.check(source, ["-Wpointer-init", "-Wno-pointer-init"]), "")

    def test_pointer_derived_component_and_local_exclusion(self):
        source = """type::node
type(node),pointer::next
end type
type(node),pointer::local
nullify(local)
end
"""
        warning = self.check(source, ["-Werror=pointer-init"], errors=True)
        self.assertEqual(warning.count("[pointer-init]"), 1, warning)
        self.assertIn("node%next", warning)

    def test_pointer_word_is_not_reserved(self):
        source = "type::item\ninteger pointer\ninteger target\nend type\nend\n"
        self.assertEqual(self.check(source, ["-Wpointer-init"]), "")

    def test_loop_subscript(self):
        source = """implicit none
integer::i,a(10)
a=0
do i=1,10
print *,a(i+1)
end do
end
"""
        warning = self.check(source, ["-Wdo-subscript"])
        self.assertEqual(warning.count("[do-subscript]"), 1, warning)
        self.assertIn("range 2:11", warning)
        self.assertIn("bounds 1:10", warning)
        self.assertIn("example.f90:5:", warning)
        self.assertIn("array declared here", warning)

    def test_loop_guards(self):
        source = """implicit none
integer::i,a(10)
a=0
do i=0,11
if(i>=1.and.i<=size(a))then
print *,a(i)
end if
if(i<1.or.i>10)cycle
print *,a(i)
end do
end
"""
        self.assertEqual(self.check(source, ["-Wdo-subscript"]), "")

    def test_loop_branch_else_and_affine_guard(self):
        source = """implicit none
integer::i,a(10)
a=0
do i=1,10
if(i+1<=10)then
print *,a(i+1)
else
print *,a(i)
end if
end do
end
"""
        self.assertEqual(self.check(source, ["-Wdo-subscript"]), "")
        self.assertEqual(self.check(source.replace("i+1<=10", "i<=size(a)-1"), ["-Wdo-subscript"]), "")

    def test_loop_triangular_bounds(self):
        source = """integer::i,j,a(10,10)
a=0
do i=1,10
do j=1,i
print *,a(i,j+1)
end do
end do
end
"""
        self.assertIn("[do-subscript]", self.check(source, ["-Wdo-subscript"]))

    def test_loop_negative_stride_and_empty(self):
        source = """implicit none
integer::i,a(10)
a=0
do i=10,1,-2
print *,a(i)
end do
do i=5,1
print *,a(i+10)
end do
end
"""
        self.assertEqual(self.check(source, ["-Wdo-subscript"]), "")
        warning = self.check(source.replace("10,1,-2", "11,1,-2"), ["-Wdo-subscript"])
        self.assertEqual(warning.count("[do-subscript]"), 1, warning)

    def test_loop_multidimensional_and_lower_bound(self):
        source = """implicit none
integer::i,j,a(0:2,2:3)
a=0
do j=2,3
do i=0,2
a(i,j+1)=1
end do
end do
print *,a
end
"""
        warning = self.check(source, ["-Wdo-subscript"])
        self.assertEqual(warning.count("[do-subscript]"), 1, warning)
        self.assertIn("dimension 2", warning)
        self.assertIn("bounds 2:3", warning)

    def test_loop_known_bad_constant_remains_error(self):
        source = "integer::a(1:2),i\na=0\ndo i=1,2\nprint *,a(3)\nend do\nend\n"
        warning = self.check(source, ["-Wdo-subscript"], errors=True)
        self.assertNotIn("[do-subscript]", warning)
        self.assertIn("error", warning.lower())

    def test_enum_exhaustiveness(self):
        source = """implicit none
enumeration type::colour
enumerator::red,green,blue
end enumeration type
type(colour)::c
c=red
select case(c)
case(red)
print *,1
case(green)
print *,2
end select
end
"""
        warning = self.check(source, ["-Wenum-exhaustive"])
        self.assertEqual(warning.count("[enum-exhaustive]"), 1, warning)
        self.assertIn("does not cover `blue`", warning)
        self.assertIn("example.f90:7:", warning)
        complete = source.replace("case(green)", "case(green:blue)")
        self.assertEqual(self.check(complete, ["-Wenum-exhaustive"]), "")
        default = source.replace("end select", "case default\nprint *,3\nend select")
        self.assertEqual(self.check(default, ["-Wenum-exhaustive"]), "")

    def test_enum_open_ranges(self):
        source = """enumeration type::colour
enumerator::red,green,blue
end enumeration type
type(colour)::c
c=red
select case(c)
case(:green)
print *,1
case(blue:)
print *,2
end select
end
"""
        self.assertEqual(self.check(source, ["-Wenum-exhaustive"]), "")

    def test_enum_interoperable_alias_values(self):
        source = """enum,bind(c)::state
enumerator::off=0,on=1,enabled=1,broken=4
end enum
type(state)::s
s=off
select case(s)
case(off)
print *,0
case(on)
print *,1
end select
end
"""
        warning = self.check(source, ["-Wenum-exhaustive"])
        self.assertIn("does not cover `broken`", warning)
        self.assertNotIn("does not cover `enabled`", warning)

    def test_enum_import_renaming(self):
        source = """module m
enumeration type::colour
enumerator::red,green,blue
end enumeration type
end module
program main
use m,only:t=>colour,r=>red,g=>green
type(t)::c
c=r
select case(c)
case(r:g)
print *,1
end select
end program
"""
        self.assertIn("does not cover `blue`", self.check(source, ["-Wenum-exhaustive"]))

    def test_unrestricted_integer_is_not_enumeration(self):
        source = "integer::i\ni=1\nselect case(i)\ncase(1)\nprint *,1\nend select\nend\n"
        self.assertEqual(self.check(source, ["-Wenum-exhaustive"]), "")

    def test_named_constant_alias_is_not_an_extra_enumerator(self):
        source = """enumeration type::colour
enumerator::red,green,blue
end enumeration type
type(colour),parameter::alias=blue
type(colour)::c
c=red
select case(c)
case(red:green)
print *,1
end select
end
"""
        warning = self.check(source, ["-Wenum-exhaustive"])
        self.assertIn("does not cover `blue`", warning)
        self.assertNotIn("`alias`", warning)

    def test_directives(self):
        source = "!#allow(precision-loss)\ndouble precision::x\nx=sqrt(2.0)\nprint *,x\nend\n"
        self.assertNotIn("[precision-loss]", self.check(source, ["-Wall"]))
        self.assertIn("[precision-loss]", self.check(source.replace("allow", "deny"), errors=True))

    def test_multifile_and_separate_module_enum_locations(self):
        with tempfile.TemporaryDirectory(prefix="value-warnings-") as tmp:
            directory = Path(tmp)
            module = directory / "colours.f90"
            main = directory / "main.f90"
            module.write_text("module colours\nenumeration type::colour\nenumerator::red,green,blue\nend enumeration type\nend module\n")
            main.write_text("use colours\ntype(colour)::c\nc=red\nselect case(c)\ncase(red)\nprint *,1\nend select\nend\n")
            result = subprocess.run([COMPILER, "-Werror=enum-exhaustive", "-c", module, main],
                                    cwd=directory, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(result.stderr.count("[enum-exhaustive]"), 1, result.stderr)
            self.assertIn("main.f90:4:", result.stderr)
            result = subprocess.run([COMPILER, "-c", module], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            result = subprocess.run([COMPILER, "-Werror=enum-exhaustive", "check", main], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(result.stderr.count("[enum-exhaustive]"), 1, result.stderr)
            self.assertIn("does not cover `green`, `blue`", result.stderr)


if __name__ == "__main__":
    unittest.main()
