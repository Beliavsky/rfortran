"""Constructor implied-DO checks: interpreter, native O0/O2, and gfortran."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class Constructors(unittest.TestCase):
    def test_values_snapshots_and_index_scope(self):
        fixture = ROOT / "tests/81_array_constructors.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="constructors-") as tmp:
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

    def test_diagnostics(self):
        cases = [
            ("integer::i,a(2),b(2)", "b=[1,2]\na=[(b,i=1,2)]", "items must currently be scalar"),
            ("integer::i,a(3)", "a=[(i,i=1,3,0)]", "stride must not be zero"),
            ("integer::i,n,a(3)", "n=3\na=[(i,i=1,n)]", "constant integers"),
            ("integer::a(3)", "a=[(i,i=1,3)]", "locally declared scalar INTEGER"),
            ("real::i\ninteger::a(3)", "a=[(i,i=1,3)]", "scalar INTEGER"),
            ("integer::i,a(3)", "a=[((i,i=1,2),i=1,2)]", "reuses index"),
            ("integer::i,a(3)", "a=[(i,i=1,3)]\nprint *,i", "uninitialized variable `i`"),
            ("integer::i,a(3)", "a=[(a(i),i=3,1,-1)]", "uninitialized"),
            ("integer::i,a(3)", "a=[(i,i=1,4)]", "shape does not conform"),
            ("integer::i,a(3)", "a=[(i,i=1,1000000)]", "expansion limit"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-constructor-") as tmp:
            path=Path(tmp)/"main.f90"
            for declarations, statements, expected in cases:
                with self.subTest(statements=statements):
                    path.write_text("program main\nimplicit none\n"+declarations+"\n"+statements+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(expected,result.stderr)

if __name__ == "__main__":
    unittest.main(verbosity=2)
