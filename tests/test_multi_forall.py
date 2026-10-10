"""Multi-index FORALL snapshots compared across interpreter, O0/O2 and gfortran."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class MultiForall(unittest.TestCase):
    def test_cartesian_products_snapshots_sections_and_specialization(self):
        fixture = ROOT / "tests/80_multi_forall.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="multi-forall-") as tmp:
            directory = Path(tmp)
            commands = build_commands(fixture, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", fixture, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout.split(), expected.split())

    def test_all_rhs_reads_precede_writes(self):
        # A sequential nested DO would initialize a(2,1) before reading it;
        # FORALL must instead diagnose its undefined value on entry.
        source = "program main\ninteger :: i,j,a(2,2)\na(1,1)=1\na(1,2)=2\nforall(i=1:2,j=1:2) a(i,j)=a(j,1)\nprint *,a\nend program main\n"
        with tempfile.TemporaryDirectory(prefix="unset-forall-") as tmp:
            path = Path(tmp) / "main.f90"
            path.write_text(source)
            result = subprocess.run([COMPILER, "check", path], capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("uninitialized", result.stderr)

if __name__ == "__main__":
    unittest.main(verbosity=2)
