"""Precision lost inside CMPLX when its KIND argument is omitted."""
import unittest
import test_unused_warnings as shared


class CmplxWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_wider_destination_does_not_restore_precision(self):
        source = """implicit none
integer, parameter :: dp=kind(1.0d0)
complex(dp) :: z
z=cmplx(1.1_dp,2.2_dp)
print *,z
end
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[cmplx-kind]"), 1, warning)
        self.assertIn("REAL(8)", warning)
        self.assertIn("COMPLEX(4)", warning)
        self.assertIn("example.f90:4:3", warning)
        self.assertIn("[cmplx-kind]", self.check(source, ["--Wall"]))
        self.assertEqual(self.check(source, ["-Wconversion", "-Wno-conversion"]), "")
        self.assertIn("[cmplx-kind]", self.check(source, ["-Werror=conversion"], errors=True))

    def test_keywords_arrays_and_output_expressions(self):
        source = """implicit none
real(kind=8) :: x(2)
x=[1.1d0,2.2d0]
print *,cmplx(y=x,x=x),abs(cmplx(x=x))
end
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[cmplx-kind]"), 2, warning)
        self.assertIn("example.f90:4:9", warning)
        self.assertIn("example.f90:4:28", warning)

    def test_specific_option_and_group_precedence(self):
        source = "print *,cmplx(1.1d0)\nend\n"
        self.assertEqual(self.check(source), "")
        for flags in [["-Wcmplx-kind"], ["--Wall"], ["-Wconversion"],
                      ["-Wcmplx-kind", "-Wno-conversion"],
                      ["-Wno-conversion", "-Wcmplx-kind"]]:
            with self.subTest(flags=flags):
                warning = self.check(source, flags)
                self.assertEqual(warning.count("[cmplx-kind]"), 1, warning)
        for flags in [["--Wall", "-Wno-cmplx-kind"],
                      ["-Wconversion", "-Wno-cmplx-kind"],
                      ["-Wno-cmplx-kind", "-Wconversion"],
                      ["--Wall", "-Wno-conversion"]]:
            with self.subTest(flags=flags):
                self.assertEqual(self.check(source, flags), "")
        for flags in [["-Werror=cmplx-kind"], ["--Wall", "--Werror"],
                      ["-Werror=conversion"]]:
            with self.subTest(flags=flags):
                self.assertIn("[cmplx-kind]", self.check(source, flags, errors=True))

    def test_specific_option_does_not_enable_other_conversion_warnings(self):
        source = """real(kind=8) :: x
real :: y
x=1.1d0
y=x
print *,y,cmplx(x)
end
"""
        warning = self.check(source, ["-Wcmplx-kind"])
        self.assertIn("[cmplx-kind]", warning)
        self.assertNotIn("[conversion]", warning)
        warning = self.check(source, ["-Wconversion", "-Wno-cmplx-kind"])
        self.assertIn("[conversion]", warning)
        self.assertNotIn("[cmplx-kind]", warning)

    def test_explicit_kind_and_exact_values(self):
        source = """implicit none
integer,parameter :: dp=kind(1.0d0)
real(dp) :: x
x=1.1_dp
print *,cmplx(x,kind=dp),cmplx(y=x,kind=4,x=x)
print *,cmplx(0.5_dp,2.0_dp),cmplx([0.5_dp,2.0_dp])
print *,cmplx(1.1,2.2),cmplx(1,2)
end
"""
        self.assertEqual(self.check(source, ["-Wconversion"]), "")

    def test_only_second_argument_narrows(self):
        warning = self.check("print *,cmplx(1.0,1.1d0)\nend\n", ["-Wconversion"])
        self.assertEqual(warning.count("[cmplx-kind]"), 1, warning)

    def test_module_kind_and_continuation(self):
        source = """module kinds
integer,parameter :: dp=kind(1.0d0)
end module
program p
use kinds,only:wp=>dp
implicit none
print *,CMPLX( &
  x=1.1_wp, y=2.2_wp)
end program
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[cmplx-kind]"), 1, warning)
        self.assertIn("example.f90:7:9", warning)

    def test_source_controls(self):
        source = """!#allow(conversion)
print *,cmplx(1.1d0)
end
"""
        self.assertEqual(self.check(source, ["-Werror=conversion"]), "")
        source = source.replace("#allow", "#deny")
        self.assertIn("[cmplx-kind]", self.check(source, errors=True))
        source = source.replace("conversion", "cmplx-kind")
        self.assertIn("[cmplx-kind]", self.check(source, errors=True))
        source = source.replace("#deny", "#allow")
        self.assertEqual(self.check(source, ["--Wall", "--Werror"]), "")

    def test_initializers_and_nested_calls(self):
        source = """complex(kind=8),parameter :: z=cmplx(1.1d0)
print *,cmplx(real(cmplx(1.1d0),kind=8))
end
"""
        warning = self.check(source, ["-Wconversion"])
        self.assertEqual(warning.count("[cmplx-kind]"), 2, warning)
        self.assertIn("example.f90:1:32", warning)
        self.assertIn("example.f90:2:20", warning)

    def test_user_procedure_is_not_intrinsic(self):
        source = """program p
implicit none
print *,cmplx(1.1d0)
contains
function cmplx(x) result(z)
real(kind=8),intent(in) :: x
complex(kind=8) :: z
z=(1.0d0,2.0d0)*x
end function
end program
"""
        self.assertEqual(self.check(source, ["-Wconversion"]), "")


if __name__ == "__main__":
    unittest.main()
