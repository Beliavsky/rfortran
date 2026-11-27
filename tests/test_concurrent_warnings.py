"""-Wcould-be-concurrent (in -Wextra): DO loops whose iterations are
independent could be DO CONCURRENT. The suggested headers are checked, and a
program rewritten as suggested must give the same output.

Run after cargo build --release: python tests/test_concurrent_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import COMPILER

PROGRAM = '''program p
implicit none
integer, parameter :: n = 4
integer :: i, j, k
real :: x(n), y(n), a(n, n), t, s
x = [(real(i), i = 1, n)]
do i = 1, n
   t = 2*x(i)
   y(i) = t + sqrt(x(i))
end do
do j = 1, n
   do i = 1, n
      a(i, j) = x(i)*y(j)
   end do
end do
do k = 1, n, 2
   y(k) = -y(k)
end do
do i = 2, n
   y(i) = y(i-1) + x(i)
end do
s = 0
do i = 1, n
   s = s + y(i)
end do
print '(4f9.4)', y
print '(4f9.4)', a
print '(f9.4)', s
end program p
'''

EXPECTED = '''  -3.0000  -1.0000   2.0000   6.0000
   3.0000   6.0000   9.0000  12.0000
   5.4142  10.8284  16.2426  21.6569
   7.7321  15.4641  23.1962  30.9282
  10.0000  20.0000  30.0000  40.0000
   4.0000
'''


def warnings(source, *options):
    with tempfile.TemporaryDirectory(prefix='concurrent-') as tmp:
        path = Path(tmp) / 'main.f90'
        path.write_text(source, encoding='utf-8')
        err = subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                             text=True, encoding='utf-8', errors='replace', timeout=120).stderr
    found = []
    for block in err.split('warning: ')[1:]:
        if '[could-be-concurrent]' not in block.split('\n')[0]:
            continue
        line = int(re.search(r':(\d+):\d+', block).group(1))
        header = re.search(r'write `([^`]*)`', block).group(1)
        found.append((line, header))
    return found


class ConcurrentWarnings(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_headers(self):
        self.assertEqual(warnings(PROGRAM, '-Wextra'), [
            (7, 'do concurrent (i = 1:n) local(t)'),
            (11, 'do concurrent (j = 1:n, i = 1:n)'),
            (16, 'do concurrent (k = 1:n:2)'),
        ])
        # Fortran 2008 has DO CONCURRENT but not LOCAL; earlier, nothing.
        self.assertEqual(warnings(PROGRAM, '-Wcould-be-concurrent', '-std=f2008')[0],
                         (7, 'do concurrent (i = 1:n)'))
        self.assertEqual(warnings(PROGRAM, '-Wcould-be-concurrent', '-std=f2003'), [])
        self.assertEqual(warnings(PROGRAM, '-Wall'), [])

    def test_rewritten_program_gives_the_same_output(self):
        self.check(PROGRAM, EXPECTED)
        lines = PROGRAM.split('\n')
        for line, header in reversed(warnings(PROGRAM, '-Wextra')):
            k = line - 1
            controls = header.count('=')
            lines[k] = header
            # Remove the inner DO statements and their END DO.
            for _ in range(controls - 1):
                del lines[k + 1]
                end = next(m for m in range(k + 1, len(lines)) if lines[m].strip() == 'end do')
                del lines[end]
        rewritten = '\n'.join(lines)
        self.assertEqual(rewritten.count('do concurrent'), 3, rewritten)
        self.check(rewritten, EXPECTED)


if __name__ == '__main__':
    unittest.main()
