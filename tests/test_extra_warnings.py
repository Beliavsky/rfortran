"""-Wextra: the opt-in advice that -Wall leaves out, except implicit-interface
and conversion. As in gfortran, it does not imply -Wall, and an option naming
a category wins over the group in either order.

Run after cargo build --bin rfort: python tests/test_extra_warnings.py
"""
import re
import unittest
import test_unused_warnings as shared

SOURCE = '''module m
contains
  subroutine s(x)
    real :: x
    integer :: k
    k = 1
    k = 2
    x = k
  end subroutine
end module
program p
  use m
  implicit none
  real :: y, data
  integer :: i
  y = 1.0
  data = 2.0
  i = 2.5d0 * y
  call s(y)
  print *, y, data, i
contains
  subroutine t
    real :: y
    y = 3.0
    print *, y
  end subroutine
end program
'''
EXTRA = ['dead-store', 'keyword-name', 'missing-intent', 'shadow']


def categories(text):
    return sorted(set(re.findall(r'^warning: .*\[([a-z-]+)\]$', text, re.M)))


class ExtraWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_group_and_controls(self):
        self.assertEqual(self.check(SOURCE), '')
        self.assertEqual(categories(self.check(SOURCE, ['-Wall'])), ['unused-function'])
        for flags in (['-Wextra'], ['--Wextra']):
            self.assertEqual(categories(self.check(SOURCE, flags)), EXTRA)
        self.assertEqual(categories(self.check(SOURCE, ['-Wall', '-Wextra'])), sorted(EXTRA + ['unused-function']))
        no_shadow = [c for c in EXTRA if c != 'shadow']
        self.assertEqual(categories(self.check(SOURCE, ['-Wextra', '-Wno-shadow'])), no_shadow)
        self.assertEqual(categories(self.check(SOURCE, ['-Wno-shadow', '-Wextra'])), no_shadow)
        self.assertEqual(self.check(SOURCE, ['-Wextra', '-Wno-extra']), '')

    def test_excluded_categories_stay_opt_in(self):
        warnings = self.check(SOURCE, ['-Wall', '-Wextra'])
        self.assertNotIn('[conversion]', warnings)
        self.assertIn('[conversion]', self.check(SOURCE, ['-Wextra', '-Wconversion']))

    def test_errors(self):
        self.assertIn('[keyword-name]', self.check(SOURCE, ['-Wextra', '-Werror=keyword-name'], errors=True))
        self.check(SOURCE, ['-Wextra', '-Werror'], errors=True)


if __name__ == '__main__':
    unittest.main()
