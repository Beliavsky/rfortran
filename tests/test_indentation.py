"""-Windentation (opt-in): construct bodies not indented past their opening
statement, statements of one body in different columns, and END, ELSE and
CASE statements not aligned with the opening statement.

Run after cargo build --release: python tests/test_indentation.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

CLEAN = '''program main
implicit none
integer :: i, k
type :: point
   real :: x, y
contains
   procedure :: norm
end type point
k = 0
outer: do i = 1, 4
   if (i > 2) then
      k = k + i
   else if (i == 1) then
      k = k - 1
   else
      k = 2*k
   end if
   select case (i)
   case (1)
      k = k + 1
   case default
      k = k + 2
   end select
   do k = 1, 1; print *, k; end do
end do outer
do 10 i = 1, 2
   k = k + i
10 continue
print *, k
contains
function norm(p) result(r)
class(point), intent(in) :: p
real :: r
r = sqrt(p%x**2 + p%y**2)
end function norm
end program main
'''

MESSY = '''program main
implicit none
integer :: i, k
k = 0
do i = 1, 4
   print *, "i", i
    print *, "i**2", i**2
end do
do i = 1, 2
k = k + i
end do
if (k > 0) then
   k = 1
  else
   k = 2
   end if
select case (k)
  case (1)
     k = 3
    case default
     k = 4
end select
type :: t
integer :: x
end type
print *, k
end program main
'''


def compile_with(source, *options):
    with tempfile.TemporaryDirectory(prefix='indentation-') as tmp:
        path = Path(tmp) / 'main.f90'
        path.write_text(source, encoding='utf-8')
        return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                              text=True, encoding='utf-8', errors='replace', timeout=120)


def warnings(err):
    """(line, message) of each indentation warning."""
    out = []
    for block in err.split('warning: ')[1:]:
        head = block.split('\n')[0]
        if '[indentation]' in head:
            line = int(re.search(r'main\.f90:(\d+):', block).group(1))
            out.append((line, head.replace(' [indentation]', '')))
    return out


class Indentation(unittest.TestCase):
    def test_clean_layout_is_not_reported(self):
        result = compile_with(CLEAN, '-Windentation')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(warnings(result.stderr), [], result.stderr)

    def test_messy_layout(self):
        err = compile_with(MESSY, '-Windentation').stderr
        self.assertEqual(warnings(err), [
            (7, 'statement is indented inconsistently with the rest of the DO loop'),
            (10, 'body of the DO loop is not indented'),
            (14, '`else` is not aligned with its IF statement'),
            (20, '`case default` is indented inconsistently with the other CASE statements'),
            (24, 'body of the derived type definition is not indented'),
        ], err)
        self.assertIn('the body starts at column 4', err)
        self.assertIn('fprettify or findent', err)

    def test_end_alignment(self):
        source = 'integer :: i\ndo i = 1, 2\n   print *, i\n   end do\nend\n'
        err = compile_with(source, '-Windentation').stderr
        self.assertEqual(warnings(err), [(4, '`end do` is not aligned with its DO statement')], err)

    def test_controls(self):
        self.assertNotIn('[indentation]', compile_with(MESSY, '-Wall', '-Wextra').stderr)
        self.assertNotIn('[indentation]', compile_with(MESSY, '-Windentation', '-Wno-indentation').stderr)
        result = compile_with(MESSY, '-Werror=indentation')
        self.assertNotEqual(result.returncode, 0)

    def test_fixed_form_is_not_checked(self):
        source = '      do 10 i = 1, 2\n      print *, i\n   10 continue\n      end\n'
        with tempfile.TemporaryDirectory(prefix='indentation-') as tmp:
            path = Path(tmp) / 'main.f'
            path.write_text(source, encoding='utf-8')
            result = subprocess.run([COMPILER, '-Windentation', 'check', path], cwd=tmp, capture_output=True,
                                    text=True, encoding='utf-8', errors='replace', timeout=120)
        self.assertNotIn('[indentation]', result.stderr)


if __name__ == '__main__':
    unittest.main()
