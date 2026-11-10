"""-Winfinite-loop, enabled by -Wall: a DO loop without loop control, or
DO WHILE (.true.), whose body has no EXIT, RETURN, STOP, branch out, or call
of a procedure that may stop. Statements after a loop that control cannot
leave are reported by -Wunreachable-code.

Run after cargo build --release: python tests/test_infinite_loop.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

NO_EXIT = '''implicit none
real :: x
do
   call random_number(x)
   print*,x
end do
end
'''

ENDING = '''program p
implicit none
integer :: i, n
i = 0
do
  i = i + 1
  if (i > 3) exit
end do
outer: do
  do
    exit outer
  end do
end do outer
do
  read (*, *, end=10) n
end do
10 continue
do
  call finish(i)
end do
contains
subroutine finish(k)
  integer, intent(in) :: k
  print *, k
  stop
end subroutine
end program p
'''

AFTER = '''implicit none
integer :: i
i = 0
do while (.true.)
  i = i + 1
  if (i > 3) stop
end do
print *, i
end
'''


class InfiniteLoop(unittest.TestCase):
    def check(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='loop-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                                  text=True, encoding='utf-8', errors='replace', timeout=120)

    def test_loop_without_exit(self):
        err = self.check(NO_EXIT, '-Wall').stderr
        self.assertIn('this DO loop has no EXIT, RETURN, STOP or branch out of it, so it never ends [infinite-loop]', err)
        self.assertIn('main.f90:3:', err)
        self.assertEqual(err.count('[infinite-loop]'), 1)

    def test_loops_that_end(self):
        result = self.check(ENDING, '-Wall')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('infinite-loop', result.stderr)

    def test_code_after_a_loop_that_only_stops(self):
        err = self.check(AFTER, '-Wall').stderr
        self.assertNotIn('[infinite-loop]', err)
        self.assertIn('unreachable statement after a loop that does not end [unreachable-code]', err)

    def test_controls(self):
        self.assertNotIn('infinite-loop', self.check(NO_EXIT).stderr)
        self.assertIn('[infinite-loop]', self.check(NO_EXIT, '-Winfinite-loop').stderr)
        self.assertNotIn('infinite-loop', self.check(NO_EXIT, '-Wall', '-Wno-infinite-loop').stderr)
        self.assertNotIn('infinite-loop', self.check('!#allow(infinite-loop)\n' + NO_EXIT, '-Wall').stderr)
        self.assertNotEqual(self.check(NO_EXIT, '-Werror=infinite-loop').returncode, 0)


if __name__ == '__main__':
    unittest.main()
