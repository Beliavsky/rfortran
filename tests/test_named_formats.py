"""Constant CHARACTER formats for WRITE and PRINT, including rejection checks."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands


class NamedFormats(unittest.TestCase):
    def test_output_and_reference(self):
        fixture = ROOT / "tests/83_named_formats.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="named-formats-") as tmp:
            directory = Path(tmp)
            for command in build_commands(fixture, directory):
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout, expected)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", fixture, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                # Explicit formats agree exactly; list-directed integer widths differ.
                self.assertEqual(result.stdout.splitlines()[:-1], expected.splitlines()[:-1])
                self.assertEqual(result.stdout.splitlines()[-1].split(), ["123"])

    def test_diagnostics(self):
        cases = [
            ("", "write (*,0) 1", "format label 0 does not label a FORMAT statement"),
            ("", "write (*,100) 1", "format label 100 does not label a FORMAT statement"),
            ("integer,parameter::fmt=0", "write (*,fmt) 1", "numeric format labels"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-formats-") as tmp:
            path = Path(tmp) / "main.f90"
            for declarations, statements, diagnostic in cases:
                with self.subTest(statements=statements):
                    path.write_text("program main\n" + declarations + "\n" + statements + "\nend program main\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "check", path], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(diagnostic, result.stderr)

    def test_once_rejected_formats(self):
        # E editing, format reversion over more items than descriptors, and a
        # WRITE to an unconnected unit, which goes to fort.20 as in gfortran.
        program = """program main
character(*),parameter::fmt='(e12.4)', f2='(i3)'
write (*,fmt) 1.0
write (*,f2) 1,2
write (20,f2) 1
end program main
"""
        with tempfile.TemporaryDirectory(prefix="named-formats-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program, encoding="utf-8")
            for command in build_commands(source, directory):
                with self.subTest(command=command):
                    (directory / "fort.20").unlink(missing_ok=True)
                    result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, "  0.1000E+01\n  1\n  2\n")
                    self.assertEqual((directory / "fort.20").read_text(), "  1\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
