"""A DO variable cannot be redefined inside its loop: by assignment, a nested
DO or implied DO with the same variable, a READ input item, or an IOSTAT=,
STAT=, NEWUNIT= or similar status variable, including through an ASSOCIATE
name. rfort reports the statement and the line of the loop, as gfortran
rejects these programs. Uses that only read the variable, keyword arguments
of user procedures named like status controls, and reuse after the loop are
accepted.

Run after cargo build --bin rfort: python tests/test_do_variable_redefinition.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

TEMPLATE = """program p
integer :: k, u, a(3)
real :: x
real, allocatable :: y(:)
u = 5
do k = 1, 3
  {statement}
end do
end program
"""

REDEFINITIONS = [
    "k = k + 1",
    "if (k > 1) k = 3",
    "read *, k",
    "read(u, *, iostat=k) x",
    "read(u, '(i3)') k",
    "read(u, *) (a(k), k = 1, 2)",
    "print *, (a(k), k = 1, 2)",
    "write(*, '(i0)', iostat=k) 1",
    "open(newunit=k, file='a')",
    "inquire(unit=3, number=k)",
    "close(3, iostat=k)",
    "allocate(y(3), stat=k)",
    "do k = 1, 2; end do",
    "do k = 1, 5, 2; end do",
    "associate (t => k); t = 2; end associate",
]

VALID = """module m
contains
  subroutine f(stat, n)
    integer, intent(in) :: stat
    integer, intent(out) :: n
    n = stat
  end subroutine
end module
program p
use m
integer :: k, j, n, a(3)
do k = 1, 3, 2
  a(k) = k
  read(k, *, iostat=j) n
  print '(4i2)', (a(j), j = 1, 3), a(k)
  call f(stat=k, n=n)
  do j = k, 3
    a(j) = k
  end do
end do
k = 7
print '(4i2)', k, a
end program
"""


def check(source_text):
    with tempfile.TemporaryDirectory(prefix="do-variable-") as tmp:
        source = Path(tmp) / "example.f90"
        source.write_text(source_text, encoding="utf-8")
        return subprocess.run([COMPILER, "check", source], cwd=tmp,
                              capture_output=True, text=True, timeout=120)


class DoVariableRedefinition(unittest.TestCase):
    def test_redefinitions_are_rejected(self):
        for statement in REDEFINITIONS:
            with self.subTest(statement=statement):
                result = check(TEMPLATE.format(statement=statement))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("DO variable `k` of the loop at line 6 cannot be redefined inside the loop",
                              result.stderr)
                self.assertIn("example.f90:7:", result.stderr)

    def test_reads_and_later_reuse_are_accepted(self):
        result = check(VALID)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
