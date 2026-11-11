"""-Wconstant-condition (in -Wextra) and -Wconstant-condition-all: conditions,
or parts of them, whose values are known at compile time.

Run after cargo build --release: python tests/test_constant_conditions.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

SOURCE = """program main
implicit none
logical, parameter :: tf = .true., debug = .false.
integer :: n, k
real :: x, y
character(len=3) :: s
logical :: flag
x = 1; y = 2; n = 3; s = "abc"
if (.true. .or. (x > y)) print *, 1
if (.false.) print *, 2
do while (.true.)
   exit
end do
if (tf .or. (x > y)) print *, 3
if (debug) print *, 4
if (n == n) print *, 5
if (x /= x) print *, 6
if (n > 5 .and. n < 3) print *, 7
if (n > 5 .or. n <= 5) print *, 8
if (n >= 1 .and. n <= 10) print *, 9
if (1 > 2) print *, 10
if (s /= s .or. x > 0) print *, 11
flag = .false. .and. x > y
k = merge(1, 2, .true. .and. n > 0)
end program main
"""


def check(*options):
    with tempfile.TemporaryDirectory(prefix='constant-') as tmp:
        path = Path(tmp) / 'main.f90'
        path.write_text(SOURCE, encoding='utf-8')
        err = subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                             text=True, encoding='utf-8', errors='replace', timeout=120).stderr
    out = []
    for block in err.split('warning: ')[1:]:
        head = block.split('\n')[0]
        category = re.search(r'\[([a-z-]+)\]$', head).group(1)
        out.append((int(re.search(r'main\.f90:(\d+):', block).group(1)), category, head.rsplit(' [', 1)[0]))
    return out


class ConstantConditions(unittest.TestCase):
    def test_default(self):
        # Not the literal conditions of lines 10 and 11, the REAL NaN test
        # of line 17, a range that may hold (line 20), or named constants.
        self.assertEqual([(l, m) for l, c, m in check('-Wextra') if c == 'constant-condition'], [
            (9, '`.true. .or. x > y` is always true; `x > y` has no effect'),
            (16, '`n == n` is always true'),
            (18, '`n > 5 .and. n < 3` is always false'),
            (19, '`n > 5 .or. n <= 5` is always true'),
            (21, '`1 > 2` is always false'),
            (22, '`s /= s` is always false and has no effect in `s /= s .or. x > 0`'),
            (23, '`.false. .and. x > y` is always false; `x > y` has no effect'),
            (24, '`.true.` has no effect in `.true. .and. n > 0`'),
        ])
        self.assertEqual([x for x in check('-Wall') if x[1].startswith('constant-condition')], [])

    def test_named_constants(self):
        got = [(l, m) for l, c, m in check('-Wconstant-condition-all') if c == 'constant-condition-all']
        self.assertIn((14, '`tf .or. x > y` is always true, since `tf` is a named constant with value .true.; `x > y` has no effect'), got)
        self.assertIn((15, '`debug` is a named constant with value .false., so the condition is always false'), got)
        # -Wconstant-condition-all includes the literal cases.
        self.assertEqual(len(got), 10, got)

    def test_if_true(self):
        global SOURCE
        saved = SOURCE
        SOURCE = """program main
implicit none
integer :: n
n = 1
if (.true.) print *, 3
if (.true.) then
   print *, 5
else
   print *, 6
end if
if (n > 1) then
   print *, 7
else if (.true.) then
   print *, 8
end if
end program main
"""
        try:
            got = [(l, m) for l, c, m in check('-Wextra') if c == 'constant-condition']
        finally:
            SOURCE = saved
        # Not `if (.true.)` with ELSE, whose ELSE -Wunreachable-code reports.
        self.assertEqual(got, [
            (5, '`if (.true.)` is always true; the IF has no effect'),
            (13, '`else if (.true.)` is always true; write `else`'),
        ])


if __name__ == '__main__':
    unittest.main()
