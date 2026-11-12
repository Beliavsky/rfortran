"""-Wmerge-guard (in -Wall) and -Wmerge-evaluation (opt-in): MERGE may
evaluate both TSOURCE and FSOURCE, whatever MASK is.

Run after cargo build --bin rfort: python tests/test_merge_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

PROGRAM = '''module m
implicit none
contains
function pick(a, i, x) result(r)
integer, intent(in) :: a(:), i
real, intent(in), optional :: x
real :: r
r = merge(real(a(i)), 0.0, i >= 1 .and. i <= size(a))
if (present(x)) r = r + merge(1.0/x, 0.0, x /= 0)
r = r + merge(r + 1, r, i > 2)
r = r + merge(a(i), 0, a(i) > 0)
r = r + merge(a(2), -1, i > 0)
end function pick
end module m
'''


class MergeWarnings(unittest.TestCase):
    def check(self, program, flags=()):
        with tempfile.TemporaryDirectory(prefix='merge-warnings-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text(program, encoding='utf-8')
            result = subprocess.run([COMPILER, *flags, 'check', source], cwd=tmp,
                                    capture_output=True, text=True, timeout=30)
            return result.returncode, result.stderr

    def test_off_by_default(self):
        self.assertEqual(self.check(PROGRAM), (0, ''))

    def test_guard_in_wall(self):
        code, stderr = self.check(PROGRAM, ['-Wall'])
        self.assertEqual(code, 0, stderr)
        self.assertEqual(stderr.count('[merge-guard]'), 2, stderr)
        self.assertNotIn('[merge-evaluation]', stderr)
        self.assertIn('the TSOURCE argument of MERGE uses `i`, which the mask tests, in a subscript or divisor', stderr)
        self.assertIn('the TSOURCE argument of MERGE uses `x`', stderr)
        self.assertIn('example.f90:8:', stderr)
        self.assertIn('example.f90:9:', stderr)
        self.assertIn('(mask ? tsource : fsource)', stderr)
        code, stderr = self.check(PROGRAM, ['-Wall', '-Wno-merge-guard'])
        self.assertNotIn('[merge-guard]', stderr)

    def test_present_guard(self):
        code, stderr = self.check('''subroutine s(x)
real, intent(in), optional :: x
print *, merge(x, 0.0, present(x))
end subroutine s
''', ['-Wmerge-guard'])
        self.assertEqual(code, 0, stderr)
        self.assertIn('references `x`, which the mask tests with PRESENT', stderr)

    def test_evaluation_opt_in(self):
        code, stderr = self.check(PROGRAM, ['-Wmerge-evaluation', '-std=f2008'])
        self.assertEqual(code, 0, stderr)
        # real(a(i)), 1.0/x and r + 1, but not a(i), a(2) or constants.
        self.assertEqual(stderr.count('[merge-evaluation]'), 3, stderr)
        self.assertNotIn('[merge-guard]', stderr)
        self.assertIn('the TSOURCE argument of MERGE is computed even when the mask does not select it', stderr)
        self.assertIn('use an IF construct, which evaluates', stderr)
        self.assertNotIn('?', stderr)
        code, stderr = self.check(PROGRAM, ['-Werror=merge-evaluation'])
        self.assertNotEqual(code, 0)
        self.assertIn('error: the TSOURCE argument of MERGE is computed', stderr)

    def test_both(self):
        # A guarded argument is reported once, as a guard.
        code, stderr = self.check(PROGRAM, ['-Wall', '-Wmerge-evaluation'])
        self.assertEqual(stderr.count('[merge-guard]'), 2, stderr)
        self.assertEqual(stderr.count('[merge-evaluation]'), 1, stderr)


if __name__ == '__main__':
    unittest.main()
