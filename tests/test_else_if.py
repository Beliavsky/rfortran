"""ELSE IF branch selection, statement boundaries, and lazy condition evaluation."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class ElseIf(unittest.TestCase):
    def check_program(self, source, cases):
        with tempfile.TemporaryDirectory(prefix="else-if-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text(source, encoding="utf-8")
            commands = build_commands(path, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", path, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                for data, expected in cases:
                    with self.subTest(command=command, data=data):
                        result = subprocess.run(command, input=data, capture_output=True, text=True, timeout=15)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(result.stderr, "")
                        self.assertEqual(result.stdout, expected)

    def test_chains_and_nested_blocks(self):
        self.check_program((ROOT / "tests/79_else_if.f90").read_text(), [("", "7\n10\n20\n30\n40\n99\n50\n60\n")])

    def test_only_reached_conditions_are_evaluated(self):
        self.check_program("""program main
implicit none
integer :: i, value
read(*,*) i
if (probe(i,1)) then
 value=10
else if (probe(i,2)) then
 value=20
elseif (probe(i,3)) then
 value=30
else
 value=40
end if
print '(i0)', value
contains
logical function probe(i,j) result(match)
integer, intent(in) :: i,j
print '(i0)', j
match=i==j
end function probe
end program main
""", [("1\n", "1\n10\n"), ("2\n", "1\n2\n20\n"), ("3\n", "1\n2\n3\n30\n"), ("4\n", "1\n2\n3\n40\n")])

    def test_invalid_chains_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="invalid-else-if-") as tmp:
            path = Path(tmp) / "main.f90"
            for body in [
                "else if (.true.) then\nend if",
                "if (.true.) then\nelse\nelse if (.false.) then\nend if",
                "if (.true.) then\nelse if (.false.)\nend if",
                "if (.true.) then\nelse if (.false.) then",
                "if (.true.) then\nelse\nif (.false.) then\nend if",
            ]:
                with self.subTest(body=body):
                    path.write_text("program main\n" + body + "\nend program main\n")
                    result = subprocess.run([COMPILER, "parse", path], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Parse:", result.stderr)

if __name__ == "__main__":
    unittest.main(verbosity=2)
