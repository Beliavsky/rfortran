"""Integer literals must fit their kind; kind-suffixed literals work as DO bounds and subscripts."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import COMPILER


class LiteralRange(unittest.TestCase):
    def reject(self, source, text):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.f90"
            path.write_text(source)
            result = subprocess.run([str(COMPILER), "run", str(path)], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("too big for default INTEGER", result.stderr)
            self.assertIn(text, result.stderr)

    def test_default_kind_literal_too_big(self):
        # A minus sign is not part of the literal, so -2147483648 is invalid too.
        for literal in ("2147483648", "-2147483648", "9223372036854775807", "-9223372036854775807",
                        "123456789012345678901234567890"):
            with self.subTest(literal=literal):
                self.reject(f"integer(8)::i\ni={literal}\nprint *,i\nend\n", f"i={literal}")
        self.reject("integer(8)::i\ndo i=1,9000000000\nend do\nend\n", "9000000000")

    def test_largest_default_and_suffixed_literals(self):
        test_deallocate.Deallocate.check(self, """implicit none
integer::j
integer(8)::i
j=2147483647
print *,j,-2147483647
i=9223372036854775807_8
print *,i,-9223372036854775807_8,02147483647
end
""", "2147483647 -2147483647\n9223372036854775807 -9223372036854775807 2147483647\n")

    def test_kind_suffixed_do_bounds_and_subscripts(self):
        test_deallocate.Deallocate.check(self, """implicit none
integer(8)::i
integer::a(2,3)
a=reshape([1,2,3,4,5,6],[2,3])
do i=1_8,5_8,2
print *,i
end do
print *,(i,i=1_8,6_8,3_8)
print *,a(2_8,3_8),a(1,2_8)
end
""", "1\n3\n5\n1 4\n6 3\n")


if __name__ == "__main__":
    unittest.main()
