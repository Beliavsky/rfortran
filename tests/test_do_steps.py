"""Explicit constant DO steps: interpreter/native O0/O2 and gfortran reference."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class DoSteps(unittest.TestCase):
    def test_execution(self):
        source = ROOT / "tests/87_do_steps.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="do-steps-") as tmp:
            directory = Path(tmp)
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, expected)
                    self.assertEqual(result.stderr, "")

    def test_diagnostics(self):
        cases = [
            ("integer::i", "1,5,0", "DO step must not be zero"),
            ("integer::i,j\nj=2", "1,5,j", "constant INTEGER"),
            ("integer::i", "1,5,1.5", "constant INTEGER"),
            ("real::i", "1,5,2", "scalar INTEGER variable"),
            ("integer::i", "1.5,5,2", "bounds must be scalar INTEGER"),
            ("integer::i", "1,5.5,2", "bounds must be scalar INTEGER"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-do-") as tmp:
            source = Path(tmp) / "main.f90"
            for decl, control, message in cases:
                with self.subTest(control=control):
                    source.write_text("program main\n"+decl+"\ndo i="+control+"\nprint *,i\nend do\nend program main\n")
                    result = subprocess.run([COMPILER, "check", source], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)

    def test_overflow(self):
        with tempfile.TemporaryDirectory(prefix="overflow-do-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            for control in ("9223372036854775807,9223372036854775807,1", "-9223372036854775807,-9223372036854775807,-2"):
                source.write_text("program main\ninteger(8)::i\ndo i="+control+"\ncycle\nend do\nend program main\n")
                for command in build_commands(source, directory):
                    with self.subTest(control=control,command=command):
                        result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn("DO variable overflow", result.stderr)

if __name__ == "__main__":
    unittest.main()

