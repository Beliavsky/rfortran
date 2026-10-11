"""Bare DO, EXIT/CYCLE, and continued headers in interpreter/native O0/O2."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, build_commands
from run_ofort_compat import equivalent


class UnboundedDo(unittest.TestCase):
    def compare(self, source, expected, directory):
        commands = build_commands(source, directory)
        if shutil.which("gfortran"):
            exe = directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands.append([exe])
        for command in commands:
            with self.subTest(command=command):
                result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assertTrue(equivalent(result.stdout, expected), result.stdout)

    def test_integer_to_string_and_nested_loops(self):
        source = ROOT / "tests/120_unbounded_do.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="unbounded-do-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_continued_control_clauses_and_return(self):
        program = """program main
implicit none
integer :: i, total
total = 0
do & ! Counted header continues onto the next logical line.
! Intervening comment.
 & i = 1, 3
  total = total + i
end do
do &
 & while (total < 8)
  total = total + 1
end do
print *, total
call finish()
contains
subroutine finish()
integer :: n
n = 8
do
  n = n + 1
  if (n == 10) then
    print *, n
    return
  end if
end do
end subroutine finish
end program main
"""
        with tempfile.TemporaryDirectory(prefix="continued-do-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "8\n10\n", directory)


if __name__ == "__main__":
    unittest.main()
