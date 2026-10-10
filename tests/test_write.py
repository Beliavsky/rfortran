"""WRITE and bare STOP checks on Windows. Run after cargo build --bin rfort.

Run: python tests/test_write.py
Set F90C_TEST_COMPILER to use the release compiler.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, build_commands


def tokens(output):
    def convert(token):
        if token in ("T", ".TRUE."): return True
        if token in ("F", ".FALSE."): return False
        try: return float(token)
        except ValueError: return token
    return [convert(token) for token in output.split()]


class WriteOutput(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="write-output-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def run_program(self, source):
        outputs = []
        for command in build_commands(source, self.directory):
            result = subprocess.run(command, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, "")
            outputs.append(result.stdout)
        return outputs

    def test_output_matches_print_and_reference(self):
        source = ROOT / "tests/74_write_stdout.f90"
        interpreted, native0, native2 = self.run_program(source)
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        self.assertEqual(native0, expected)
        self.assertEqual(native2, expected)
        # Existing list-directed real formatting uses more trailing zeros in the interpreter.
        self.assertEqual(tokens(interpreted), tokens(expected))
        self.assertEqual(interpreted.splitlines().count(""), 2)
        if shutil.which("gfortran"):
            exe = self.directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(tokens(native0), tokens(result.stdout))
            self.assertEqual(native0.splitlines()[5], result.stdout.splitlines()[5])
            self.assertEqual(result.stdout.splitlines().count(""), 2)

    def test_stop_terminates_program_from_each_context(self):
        programs = ["stop", "if (.true.) stop", "block\nstop\nend block",
                    "block\ninteger :: i\ndo i=1,3\nif (i==2) stop\nend do\nend block",
                    "call finish()"]
        for statement in programs:
            with self.subTest(statement=statement):
                source = self.directory / "stop.f90"
                procedures = "contains\nsubroutine finish()\nstop\nend subroutine finish\n" if statement == "call finish()" else ""
                source.write_text("write(*,*) 'before'\n" + statement + "\nwrite(*,*) 'after'\n" + procedures + "end\n")
                for output in self.run_program(source):
                    self.assertEqual(output.strip(), "before")

    def test_write_remains_a_variable_array_and_procedure_name(self):
        programs = [
            ("integer :: write(2)\nwrite=[4,5]\nwrite(*,*) write(1),write(2)\nend", "4 5"),
            ("write(*,*) write(3)\ncontains\ninteger function write(i) result(v)\ninteger :: i\nv=i+1\nend function write\nend", "4"),
            ("call write()\ncontains\nsubroutine write()\nwrite(*,*) 9\nend subroutine write\nend", "9"),
        ]
        for program, expected in programs:
            source = self.directory / "identifier.f90"
            source.write_text(program)
            for output in self.run_program(source):
                self.assertEqual(output.strip(), expected)


if __name__ == "__main__":
    unittest.main(verbosity=2)
