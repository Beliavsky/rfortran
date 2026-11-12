"""-Wrecursive-io (on by default), -Wunit-leak (in -Wall),
-Wpreconnected-unit (in -Wextra) and -Wfunction-io (in -Wstyle).

Run after cargo build --bin rfort: python tests/test_io_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

FUNC_PRINT = '''module m
implicit none
contains
real function func(x)
real, intent(in) :: x
if (x < 0) then
   print*,"in func, x =",x
   func = 0.0
else
   func = sqrt(x)
end if
end function func
end module m

use m
implicit none
print*,func(4.0)
print*,func(-4.0)
end
'''

ALLOWED = '''module m
implicit none
contains
real function f(x)
real, intent(in) :: x
character(len=20) :: s
write (s,'(f8.3)') x
write (0,*) "debug", x
f = 2*x
end function f
real function g(x)
real, intent(in) :: x
call helper(x)
g = x
end function g
subroutine helper(x)
real, intent(in) :: x
print *, "helper", x
end subroutine helper
end module m
use m
implicit none
real :: y
y = f(1.0)
print *, y, f(2.0)
print *, g(3.0)
end
'''

WRITE_SUB = '''module m
implicit none
contains
subroutine sub(x)
integer :: iu
real, intent(in) :: x
open (newunit=iu, file="temp.txt")
write (iu,*) x, x**2
end subroutine sub
end module m

use m
implicit none
call sub(4.0)
end
'''

KEPT_UNITS = '''module m
implicit none
integer :: log_unit
contains
subroutine a()
integer :: iu
open (newunit=iu, file="a.txt")
write (iu,*) 1
close (iu)
end subroutine a
subroutine b()
integer, save :: iu
open (newunit=iu, file="b.txt")
end subroutine b
subroutine c(iu)
integer, intent(out) :: iu
open (newunit=iu, file="c.txt")
end subroutine c
subroutine d()
open (newunit=log_unit, file="d.txt")
end subroutine d
subroutine e()
integer :: iu
open (newunit=iu, file="e.txt")
call keep(iu)
end subroutine e
subroutine keep(iu)
integer, intent(in) :: iu
log_unit = iu
end subroutine keep
integer function h() result(iu)
open (newunit=iu, file="h.txt")
end function h
end module m
program p
use m
integer :: iu
open (newunit=iu, file="p.txt")
end program p
'''

UNITS = '''implicit none
integer, parameter :: out = 6
integer :: i
print*,"enter an integer"
read (5,*) i
write (6,*) "square is", i**2
write (out,*) "again"
write (0,*) "error"
end
'''

PORTABLE_UNITS = '''program p
use iso_fortran_env, only: output_unit, input_unit
implicit none
integer :: i
open (6, file="out.txt")
write (6,*) "to file"
write (*,*) "star"
write (output_unit,*) "ok"
read (input_unit,*,iostat=i) i
write (7,*) "seven"
end program p
'''


class IoWarnings(unittest.TestCase):
    def check(self, program, flags=()):
        with tempfile.TemporaryDirectory(prefix='io-warnings-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text(program, encoding='utf-8')
            result = subprocess.run([COMPILER, *flags, 'check', source], cwd=tmp,
                                    capture_output=True, text=True, timeout=30)
            return result.returncode, result.stderr

    def test_recursive_io_on_by_default_and_controls(self):
        code, stderr = self.check(FUNC_PRINT)
        self.assertEqual(code, 0, stderr)
        # One warning for the inner PRINT, labelling both references.
        self.assertEqual(stderr.count('[recursive-io]'), 1, stderr)
        self.assertIn('PRINT on standard output in `func` can run while another input/output statement', stderr)
        self.assertIn('example.f90:7:', stderr)
        self.assertEqual(stderr.count('`func` is referenced during this PRINT'), 2, stderr)
        self.assertIn('Recursive I/O not allowed', stderr)
        self.assertEqual(self.check(FUNC_PRINT, ['-Wno-recursive-io']), (0, ''))
        self.assertEqual(self.check('!#allow(recursive-io)\n' + FUNC_PRINT), (0, ''))
        code, stderr = self.check(FUNC_PRINT, ['-Werror=recursive-io'])
        self.assertNotEqual(code, 0)
        self.assertIn('error: PRINT on standard output', stderr)

    def test_recursive_io_through_calls_and_allowed_io(self):
        code, stderr = self.check(ALLOWED)
        self.assertEqual(code, 0, stderr)
        # An internal file and unit 0 are other units; f is evaluated before
        # the PRINT in y = f(1.0). helper's PRINT runs during the PRINT of g.
        self.assertEqual(stderr.count('[recursive-io]'), 1, stderr)
        self.assertIn('PRINT on standard output in `helper`, which `g` calls, can run', stderr)
        self.assertIn('`g` is referenced during this PRINT', stderr)

    def test_recursive_io_same_external_unit(self):
        program = '''module m
implicit none
integer, parameter :: log = 20
contains
integer function f(n)
integer, intent(in) :: n
write (log,*) "f", n
write (30,*) "other unit"
f = n
end function f
end module m
use m
implicit none
open (log, file="log.txt")
open (30, file="other.txt")
write (log,*) f(1)
end
'''
        code, stderr = self.check(program)
        self.assertEqual(code, 0, stderr)
        self.assertEqual(stderr.count('[recursive-io]'), 1, stderr)
        self.assertIn('WRITE on unit 20 in `f`', stderr)

    def test_unit_leak(self):
        self.assertEqual(self.check(WRITE_SUB), (0, ''))
        for flags in (['-Wall'], ['-Wunit-leak']):
            code, stderr = self.check(WRITE_SUB, flags)
            self.assertEqual(code, 0, stderr)
            self.assertEqual(stderr.count('[unit-leak]'), 1, stderr)
            self.assertIn('unit `iu` opened with NEWUNIT= is never closed in `sub`', stderr)
            self.assertIn('example.f90:7:', stderr)
            self.assertIn('add close(iu)', stderr)
        self.assertNotIn('[unit-leak]', self.check(WRITE_SUB, ['-Wall', '-Wno-unit-leak'])[1])
        code, stderr = self.check(WRITE_SUB, ['-Werror=unit-leak'])
        self.assertNotEqual(code, 0)
        self.assertIn('error: unit `iu`', stderr)

    def test_unit_leak_exemptions(self):
        # CLOSE, SAVE, a dummy, a module variable, a unit passed on, a
        # function result and a main program are not reported.
        code, stderr = self.check(KEPT_UNITS, ['-Wunit-leak'])
        self.assertNotIn('[unit-leak]', stderr)

    def test_preconnected_unit(self):
        self.assertNotIn('[preconnected-unit]', self.check(UNITS)[1])
        self.assertNotIn('[preconnected-unit]', self.check(UNITS, ['-Wall'])[1])
        for flags in (['-Wextra'], ['-Wpreconnected-unit']):
            code, stderr = self.check(UNITS, flags)
            self.assertEqual(code, 0, stderr)
            self.assertEqual(stderr.count('[preconnected-unit]'), 4, stderr)
            self.assertIn('READ from unit 5 assumes it is preconnected to standard input', stderr)
            self.assertIn('WRITE to unit 6 assumes it is preconnected to standard output', stderr)
            self.assertIn('WRITE to unit 0 assumes it is preconnected to standard error', stderr)
            self.assertIn('use * or INPUT_UNIT from ISO_FORTRAN_ENV', stderr)
            self.assertIn('use ERROR_UNIT from ISO_FORTRAN_ENV', stderr)
            self.assertIn('or define `out` as OUTPUT_UNIT', stderr)
        # Before Fortran 2003 there is no ISO_FORTRAN_ENV and no portable unit 0.
        code, stderr = self.check(UNITS, ['-Wpreconnected-unit', '-std=f95'])
        self.assertEqual(stderr.count('[preconnected-unit]'), 3, stderr)
        self.assertNotIn('ISO_FORTRAN_ENV', stderr)
        self.assertNotIn('[preconnected-unit]', self.check(UNITS, ['-Wextra', '-Wno-preconnected-unit'])[1])

    def test_preconnected_unit_exemptions(self):
        code, stderr = self.check(PORTABLE_UNITS, ['-Wpreconnected-unit'])
        self.assertNotIn('[preconnected-unit]', stderr)

    def test_function_io(self):
        self.assertNotIn('[function-io]', self.check(FUNC_PRINT, ['-Wall', '-Wextra'])[1])
        for flags in (['-Wstyle'], ['-Wfunction-io']):
            code, stderr = self.check(FUNC_PRINT, flags)
            self.assertEqual(stderr.count('[function-io]'), 1, stderr)
            self.assertIn('function `func` performs input/output on an external unit', stderr)
            self.assertIn('cannot be PURE', stderr)
        # f writes to unit 0 as well as an internal file; g only calls.
        code, stderr = self.check(ALLOWED, ['-Wfunction-io'])
        self.assertEqual(stderr.count('[function-io]'), 1, stderr)
        self.assertIn('function `f` performs', stderr)
        internal = 'module m\ncontains\nfunction f(x) result(s)\nreal, intent(in) :: x\ncharacter(len=12) :: s\nwrite (s,"(f12.3)") x\nend function f\nend module m\n'
        self.assertNotIn('[function-io]', self.check(internal, ['-Wfunction-io'])[1])


if __name__ == '__main__':
    unittest.main()
