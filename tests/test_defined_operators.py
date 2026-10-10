"""Defined operator execution and diagnostics; build rfort before running."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, COMPILER, build_commands


class DefinedOperators(unittest.TestCase):
    def test_execution_and_precedence(self):
        source = ROOT / "tests/76_defined_operators.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="defined-operators-") as tmp:
            directory = Path(tmp)
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                reference = directory / "reference.exe"
                compiled = subprocess.run(["gfortran", source, "-o", reference], cwd=directory,
                                          capture_output=True, text=True, timeout=60)
                self.assertEqual(compiled.returncode, 0, compiled.stderr)
                commands.append([reference])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout, expected)

    def test_pure_caller_cannot_hide_impure_function_behind_operator(self):
        source = (ROOT / "tests/76_defined_operators.f90").read_text()
        source = source.replace("pure integer function add_int", "integer function add_int")
        with tempfile.TemporaryDirectory(prefix="defined-purity-") as tmp:
            path = Path(tmp) / "impure.f90"
            path.write_text(source)
            for command in ([COMPILER, path], [COMPILER, "run", path]):
                result = subprocess.run(command, capture_output=True, text=True, timeout=60)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("PURE procedure cannot call impure or unknown procedure `add_int`", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
