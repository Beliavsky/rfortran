"""GNU extension procedures: CALL FLUSH, SLEEP, EXIT and ABORT run; -std
rejects every GNU extension rfort provides, and REAL*8 style declarations,
naming the standard replacement; -Wgnu-extension, in -Wall, warns about the
same procedures without -std. A program's own procedure of the same name is
called and not reported.

Each runnable case is checked in the interpreter, at -O0 and -O2, and
against gfortran -std=gnu.
Run after cargo build --bin rfort: python tests/test_gnu_extensions.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import COMPILER, build_commands

EXTENSIONS = """program p
implicit none
real*8 :: x
integer*4 :: n
complex*16 :: z
character(len=200) :: d, a
integer :: st
x = dfloat(2)
z = dcmplx(1d0, 2d0)
z = dconjg(z)
n = 1
print *, isnan(x), x, z, n
call getarg(0, a)
call getcwd(d)
call system("exit 0", st)
call flush(6)
call sleep(0)
if (st > 5) call abort()
call exit(0)
end program
"""

# The message for each extension, after its name.
ADVICE = {
    "DFLOAT": "use DBLE",
    "DCMPLX": "use CMPLX(x, y, kind(1d0))",
    "DCONJG": "use CONJG",
    "ISNAN": "use IEEE_IS_NAN from the intrinsic module IEEE_ARITHMETIC",
    "CALL GETARG": "use GET_COMMAND_ARGUMENT(number, value)",
    "CALL GETCWD": "the standard has no equivalent",
    "CALL SYSTEM": "use EXECUTE_COMMAND_LINE(command, exitstat=status)",
    "CALL FLUSH": "use the FLUSH statement, FLUSH(unit)",
    "CALL SLEEP": "the standard has no equivalent",
    "CALL ABORT": "use ERROR STOP",
    "CALL EXIT": "use STOP code",
}


class GnuExtensions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def compile(self, source, *options):
        with tempfile.TemporaryDirectory(prefix="gnu-extensions-") as tmp:
            path = Path(tmp) / "p.f90"
            path.write_text(source, encoding="utf-8")
            return subprocess.run([COMPILER, *options, "--quiet", path, "-o", Path(tmp) / "p.exe"],
                                  cwd=tmp, capture_output=True, text=True, timeout=120)

    def test_standard_rejects_extensions(self):
        result = self.compile(EXTENSIONS, "-std=f2018")
        self.assertNotEqual(result.returncode, 0)
        for name, advice in ADVICE.items():
            self.assertIn(f"{name} is a GNU extension, not part of Fortran 2018; {advice}", result.stderr)
        for written, standard in [("real*8", "real(real64) with ISO_FORTRAN_ENV, or real(kind=8)"),
                                  ("integer*4", "integer(int32) with ISO_FORTRAN_ENV, or integer(kind=4)"),
                                  ("complex*16", "complex(real64) with ISO_FORTRAN_ENV, or complex(kind=8)")]:
            self.assertIn(f"`{written}` is an extension, not part of Fortran 2018; write {standard}", result.stderr)
        self.assertIn("standard compatibility checks failed", result.stderr)

    def test_wall_warns_about_extension_procedures(self):
        result = self.compile(EXTENSIONS, "-Wall")
        self.assertEqual(result.returncode, 0, result.stderr)
        for name, advice in ADVICE.items():
            self.assertIn(f"{name} is a GNU extension; {advice} [gnu-extension]", result.stderr)
        # Declarations such as REAL*8 are -Wliteral-kind advice, not this.
        self.assertNotIn("real*8", result.stderr)
        result = self.compile(EXTENSIONS, "-Wall", "-Wno-gnu-extension")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("[gnu-extension]", result.stderr)
        result = self.compile(EXTENSIONS)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("GNU extension", result.stderr)
        result = self.compile(EXTENSIONS, "-Werror=gnu-extension")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("gnu-extension warnings treated as errors", result.stderr)

    def test_own_procedures_are_not_extensions(self):
        source = """module m
contains
logical function isnan(x)
real, intent(in) :: x
isnan = x /= x
end function
end module
program p
use m
implicit none
external system
interface
  subroutine sleep(n)
    integer, intent(in) :: n
  end subroutine
end interface
print *, isnan(1.0), dfloat(3)
call system('x')
call sleep(1)
call flush(6)
contains
real function dfloat(i)
integer, intent(in) :: i
dfloat = i
end function
end program
subroutine system(c)
character(*) :: c
print *, 'own system ', c
end subroutine
subroutine sleep(n)
integer, intent(in) :: n
print *, 'own sleep', n
end subroutine
subroutine flush(u)
integer :: u
print *, 'own flush', u
end subroutine
"""
        for options in (["-std=f2018"], ["-Wall"]):
            with self.subTest(options=options):
                result = self.compile(source, *options)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn("extension", result.stderr)
        self.check(source, "F 3.0\nown system x\nown sleep 1\nown flush 6\n", reference=False)

    def test_flush_and_sleep(self):
        # Both were rejected as undefined subroutines.
        self.check("""program p
implicit none
print *, 'a'
call flush(6)
call flush()
call sleep(0)
print *, 'b'
end program
""", "a\nb\n", std="gnu")

    def test_sleep_waits(self):
        source = "program p\ncall sleep(1)\nprint *, 'done'\nend program\n"
        with tempfile.TemporaryDirectory(prefix="gnu-sleep-") as tmp:
            path = Path(tmp) / "p.f90"
            path.write_text(source, encoding="utf-8")
            for command in build_commands(path, Path(tmp)):
                with self.subTest(command=command):
                    start = time.monotonic()
                    result = subprocess.run(command, cwd=tmp, capture_output=True, text=True, timeout=60)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.split(), ["done"])
                    # Compiled commands build first, so only the lower bound holds.
                    self.assertGreaterEqual(time.monotonic() - start, 0.9)

    def test_exit_and_abort_status(self):
        cases = [
            ("program p\ninteger :: code\ncode = 4\nprint *, 'a'\nif (code > 3) call exit(code)\nprint *, 'b'\nend program\n",
             4, ["a"], ""),
            ("program p\nprint *, 'a'\ncall exit()\nprint *, 'b'\nend program\n", 0, ["a"], ""),
            ("program p\nprint *, 'a'\ncall abort()\nprint *, 'b'\nend program\n", 1, ["a"], "Program aborted"),
        ]
        for source, code, stdout, stderr in cases:
            with tempfile.TemporaryDirectory(prefix="gnu-exit-") as tmp:
                path = Path(tmp) / "p.f90"
                path.write_text(source, encoding="utf-8")
                for command in build_commands(path, Path(tmp)):
                    with self.subTest(source=source, command=command):
                        result = subprocess.run(command, cwd=tmp, capture_output=True, text=True, timeout=60)
                        self.assertEqual(result.returncode, code, result.stderr)
                        self.assertEqual(result.stdout.split(), stdout)
                        if stderr:
                            self.assertIn(stderr, result.stderr)
                        else:
                            # EXIT stops silently, unlike STOP code.
                            self.assertNotIn("STOP", result.stderr)


if __name__ == "__main__":
    unittest.main()
