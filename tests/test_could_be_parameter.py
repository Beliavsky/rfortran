"""-Wcould-be-parameter, opt-in: a local scalar defined once from a constant
expression and never modified could be a named constant. The cases follow
xparam.py's rules and test programs.

Run after cargo build --release: python tests/test_could_be_parameter.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

XAREA = '''implicit none
real :: radius, pi
pi = 3.14
call random_number(radius)
print*,"radius, area =", radius, pi*radius**2
end
'''

XPARAM = '''program xparam
! test program in which some variables can be made parameters
implicit none
integer :: n
real :: x1, x2, x3
real, allocatable :: x4(:)
character (len=10) :: msg
x1 = 3.1
x2 = 2*x1
call random_number(x3)
n = 5
allocate (x4(n))
msg = "hello"
end program xparam
'''

XLOOP = '''program main
implicit none
real :: x, y
integer :: i
x = 2.1
do i=1,3
   y = real(i)
end do
end program main
'''

PROCEDURES = '''module kinds
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
subroutine show(label, value)
character(len=*), intent(in) :: label
real(dp), intent(in) :: value
print '(a, f8.4)', label, value
end subroutine
subroutine bump(value)
real(dp), intent(inout) :: value
value = value + 1
end subroutine
end module kinds

program main
use kinds
implicit none
real(dp) :: pi = 3.141592653589793_dp
real(dp) :: tau, e, counter
integer :: unit_count, ios
tau = 2*pi
e = exp(1.0_dp)
counter = 0
unit_count = 0
call show("tau", tau)
call bump(counter)
write (*, '(i0)', iostat=ios) unit_count
call report()
contains
subroutine report()
call show("e", e)
unit_count = 1
end subroutine
end program main
'''


class CouldBeParameter(unittest.TestCase):
    def check(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='param-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp,
                                  capture_output=True, text=True, timeout=120)

    def reports(self, source):
        """Each reported variable and its suggestion."""
        err = self.check(source, '-Wcould-be-parameter').stderr
        names = re.findall(r'`(\w+)` is (?:assigned once|initialized)', err)
        suggestions = re.findall(r'suggestion: (.*)', err)
        return dict(zip(names, (s.strip() for s in suggestions))), err

    def test_xarea(self):
        reports, err = self.reports(XAREA)
        self.assertEqual(reports, {'pi': 'real, parameter :: pi = 3.14'}, err)
        self.assertIn('and remove the assignment', err)
        self.assertIn('declare `pi` in a separate statement', err)
        self.assertIn('[could-be-parameter]', err)

    def test_xparam(self):
        reports, err = self.reports(XPARAM)
        self.assertEqual(reports, {
            'n': 'integer, parameter :: n = 5',
            'x1': 'real, parameter :: x1 = 3.1',
            'x2': 'real, parameter :: x2 = 2*x1',
            'msg': 'character (len=10), parameter :: msg = "hello"',
        }, err)

    def test_loop_and_procedures(self):
        reports, err = self.reports(XLOOP)
        self.assertEqual(reports, {'x': 'real, parameter :: x = 2.1'}, err)
        # counter is INTENT(INOUT), unit_count is changed by report and IOSTAT=
        # defines ios; tau and e only reach INTENT(IN) dummies.
        reports, err = self.reports(PROCEDURES)
        self.assertEqual(reports, {
            'pi': 'real(dp), parameter :: pi = 3.141592653589793_dp',
            'tau': 'real(dp), parameter :: tau = 2*pi',
            'e': 'real(dp), parameter :: e = exp(1.0_dp)',
        }, err)
        self.assertIn('`pi` is initialized with a constant and never modified', err)

    def test_entity_lengths_arrays_and_implied_do(self):
        source = '''program p
implicit none
character(2) :: a*3, b
integer :: i, m(2,2) = 7, k
a = 'hello'
b = 'world'
k = 3
print *, a, b, m, [(k*i, i=1,3)]
end program p
'''
        reports, err = self.reports(source)
        self.assertEqual(reports, {
            'a': "character(2), parameter :: a*3 = 'hello'",
            'b': "character(2), parameter :: b = 'world'",
            'k': 'integer, parameter :: k = 3',
        }, err)

    def test_suggestions_compile(self):
        # xarea with the suggestion applied compiles without warnings.
        fixed = XAREA.replace('real :: radius, pi\npi = 3.14\n', 'real :: radius\nreal, parameter :: pi = 3.14\n')
        result = self.check(fixed, '-Wcould-be-parameter')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('could-be-parameter', result.stderr)

    def test_opt_in_and_controls(self):
        for options in ((), ('-Wall',)):
            self.assertNotIn('could-be-parameter', self.check(XAREA, *options).stderr, options)
        result = self.check(XAREA, '-Werror=could-be-parameter')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('could-be-parameter', self.check(XAREA, '-Wcould-be-parameter', '-Wno-could-be-parameter').stderr)


if __name__ == '__main__':
    unittest.main()
