"""Runtime format strings: interpreter, native O0/O2, and gfortran comparison."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands


class RuntimeFormats(unittest.TestCase):
    def test_changing_formats(self):
        fixture = ROOT / "tests/84_runtime_formats.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="runtime-format-") as tmp:
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
                    self.assertEqual(result.stdout, expected)
                    self.assertEqual(result.stderr, "")

    def test_formats_read_from_input(self):
        with tempfile.TemporaryDirectory(prefix="input-format-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text("""program main
character(200) :: fmt
read (*,'(a)') fmt
write (*,trim(fmt)) 7,1.25,.true.,'ab'
end program main
""")
            commands = build_commands(source, directory)
            for command in commands:
                for fmt, expected in [
                    ("(i3,f6.2,l2,a4)", "  7  1.25 T  ab\n"),
                    ("(i0,1x,f5.1,1x,l1,1x,a1)", "7   1.2 T a\n"),
                    ("(i3,f6.2,l2,a4,i3)", "  7  1.25 T  ab\n"),
                ]:
                    with self.subTest(command=command, fmt=fmt):
                        result = subprocess.run(command, input=fmt+"\n", capture_output=True, text=True, timeout=15)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(result.stdout, expected)
                        self.assertEqual(result.stderr, "")
                for fmt, message in [
                    ("i3", "parenthesized"),
                    ("(i3,e10.2,l2,a4)", "unsupported output descriptor"),
                    ("(i3,f6.2,l2,i4)", "item type"),
                    ("(a3,f6.2,l2,a4)", "item type"),
                    ("(i3,f6.2,l2)", "descriptor count"),
                    ("(i999999,f6.2,l2,a4)", "supported limits"),
                    ("(999999999999999999999999i1)", "supported limits"),
                    ("('unterminated)", "unterminated literal"),
                    ("(i3,f6,l2,a4)", "precision"),
                    ("(2(i3)", "unterminated output format group"),
                    ("(i3,f6.2,l2,a4) trailing", "parenthesized"),
                ]:
                    with self.subTest(command=command, fmt=fmt):
                        result = subprocess.run(command, input=fmt+"\n", capture_output=True, text=True, timeout=15)
                        self.assertNotEqual(result.returncode, 0)
                        self.assertEqual(result.stdout, "")
                        self.assertIn("Runtime error: invalid output format", result.stderr)
                        self.assertIn(message, result.stderr)

    def test_compile_time_checks(self):
        with tempfile.TemporaryDirectory(prefix="bad-runtime-format-") as tmp:
            source = Path(tmp) / "main.f90"
            for body, message in [
                ("character(8)::fmt\nwrite (*,fmt) 7", "uninitialized variable `fmt`"),
                ("integer::fmt\nfmt=10\nwrite (*,fmt) 7", "scalar CHARACTER"),
            ]:
                with self.subTest(body=body):
                    source.write_text("program main\n"+body+"\nend program main\n")
                    result = subprocess.run([COMPILER, "check", source], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
