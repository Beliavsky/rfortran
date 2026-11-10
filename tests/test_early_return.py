"""-Wearly-return (opt-in): RETURN statements after which statements of the
procedure can still run. A RETURN that is the last executable statement, or
is followed only by END IF, END SELECT, CONTINUE and other branches of its
constructs, is not reported.

Run after cargo build --release: python tests/test_early_return.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

SOURCE = '''module m
implicit none
contains
subroutine scale(x, n)
real, intent(inout) :: x(:)
integer, intent(in) :: n
integer :: i
if (n <= 0) return
do i = 1, n
   if (x(i) < 0) return
   x(i) = 2*x(i)
end do
return
end subroutine scale
integer function sign_of(k)
integer, intent(in) :: k
if (k > 0) then
   sign_of = 1
   return
else if (k < 0) then
   sign_of = -1
   return
end if
sign_of = 0
end function sign_of
integer function kind_of(k)
integer, intent(in) :: k
select case (k)
case (1)
   kind_of = 10
   return
case default
   kind_of = 20
end select
end function kind_of
end module m
program main
use m
implicit none
real :: x(3) = [1.0, -1.0, 3.0]
call scale(x, 3)
print *, x, sign_of(-4), sign_of(0), kind_of(1)
end program main
'''

FIXED = '''      subroutine f77(n, s)
      integer n, i, s
      s = 0
      do 10 i = 1, n
         if (i .gt. 5) return
         s = s + i
   10 continue
      return
      entry g77(n, s)
      s = -n
      return
      end
'''


def compile_with(name, source, *options):
    with tempfile.TemporaryDirectory(prefix='early-return-') as tmp:
        path = Path(tmp) / name
        path.write_text(source, encoding='utf-8')
        return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                              text=True, encoding='utf-8', errors='replace', timeout=120)


def lines(err, name='main.f90'):
    return [int(re.search(re.escape(name) + r':(\d+):', b).group(1))
            for b in err.split('warning: ')[1:] if '[early-return]' in b.split('\n')[0]]


class EarlyReturn(unittest.TestCase):
    def test_early_returns(self):
        err = compile_with('main.f90', SOURCE, '-Wearly-return').stderr
        # The guard clause, the RETURN in the loop, and the RETURN in an IF
        # block followed by `sign_of = 0`; not the final RETURN of `scale`,
        # nor the RETURN in a CASE followed only by other cases.
        self.assertEqual(lines(err), [8, 10, 19, 22], err)
        self.assertIn('RETURN before the end of subroutine `scale` [early-return]', err)
        self.assertIn('RETURN before the end of function `sign_of`', err)
        self.assertIn('it also ends the enclosing loop', err)

    def test_fixed_form(self):
        err = compile_with('f77.f', FIXED, '-Wearly-return').stderr
        # The RETURN in the labeled loop; the RETURN before ENTRY is needed.
        self.assertEqual(lines(err, 'f77.f'), [5], err)

    def test_controls(self):
        self.assertNotIn('[early-return]', compile_with('main.f90', SOURCE, '-Wall', '-Wextra').stderr)
        self.assertNotIn('[early-return]', compile_with('main.f90', SOURCE, '-Wearly-return', '-Wno-early-return').stderr)
        self.assertNotEqual(compile_with('main.f90', SOURCE, '-Werror=early-return').returncode, 0)


if __name__ == '__main__':
    unittest.main()
