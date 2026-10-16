"""Read-only sequential file checks for the interpreter and native O0/O2."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, COMPILER, build_commands


def literal(path):
    return "'" + str(path).replace("'", "''") + "'"


class FileIO(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="rfort files (A&B) ")
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.data = self.directory / "input records.txt"

    def source(self, text):
        path = self.directory / "main.f90"
        path.write_text(text, encoding="utf-8")
        return path

    def run_modes(self, source, expected=None, error=None):
        for command in build_commands(source, self.directory):
            with self.subTest(command=command):
                result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                if error:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error, result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout, expected)

    def test_positioning_matches_reference_for_lf_crlf_and_partial_final_record(self):
        fixture = ROOT / "tests/77_file_positioning.f90"
        source = self.source(fixture.read_text().replace("'tests/io/file_records.txt'", literal(self.data)))
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        commands = build_commands(source, self.directory)
        if shutil.which("gfortran"):
            exe = self.directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands.append([exe])
        for data in [b"alpha\n\n123456789\nlast\n", b"alpha\r\n\r\n123456789\r\nlast\r\n", b"alpha\n\n123456789\nlast"]:
            self.data.write_bytes(data)
            for command in commands:
                with self.subTest(data=data, command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout, expected)

    def test_runtime_failures(self):
        opening = f"open(20,file={literal(self.data)},status='old',action='read')\n"
        for data, statement, error in [
            (None, opening, "OPEN failed"),
            (b"", opening+"read(20,'(a)') text", "end of file or input error"),
            (b"a"*12000+b"\xff\n", opening+"read(20,'(a)') text", "non-NUL ASCII"),
            (b"abc\n", "read(20,'(a)') text", "file unit is not connected"),
            (b"abc\n", opening+"close(20)\nread(20,'(a)') text", "file unit is not connected"),
            (b"abc\n", "backspace(20)", "file unit is not connected"),
            (b"abc\n", opening+opening, "already connected"),
            (b"abc\n", "iu=1024\nrewind(iu)", "file units must be in 0..1023"),
        ]:
            if data is not None:
                self.data.write_bytes(data)
            source = self.source("character(len=3)::text\ninteger::iu\n"+statement+"\nend\n")
            self.run_modes(source, error=error)

    def test_zero_length_and_long_records(self):
        self.data.write_bytes(b"x"*12000+b"\r\nbeta\r\n")
        source = self.source(f"""character(len=0)::empty
character(len=5)::text
open(20,file={literal(self.data)},status='old',action='read')
read(20,'(a)') empty
read(20,'(a)') text
print *, '['//text//']'
backspace(20)
backspace(20)
read(20,'(a)') text
print *, '['//text//']'
close(20)
end
""")
        self.run_modes(source, " [beta ]\n [xxxxx]\n")

    def test_contextual_names(self):
        source = self.source("""integer::open(2),close,backspace,rewind
open(1)=10
open(2)=20
close=3
backspace=4
if (.true.) rewind=5
print '(5i3)',open(1),open(2),close,backspace,rewind
end
""")
        self.run_modes(source, " 10 20  3  4  5\n")

    def test_compile_time_diagnostics(self):
        for source, error in [
            ("open(20,file='data',status='replace',action='read')\nend", "unsupported OPEN STATUS/ACTION combination"),
            ("open(20,file='data',status='old',action='read',iostat=i)\nend", "unsupported I/O control `iostat`"),
            ("open(20,unit=21,file='data',status='old',action='read')\nend", "duplicate unit"),
            ("close(20,status='delete')\nend", "supports only UNIT"),
            ("integer::iu\nrewind(iu)\nend", "uninitialized variable `iu`"),
            ("character::text\ninteger::iu\nread(iu,'(a)') text\nend", "uninitialized variable `iu`"),
            ("rewind(5)\nend", "excluding preconnected units"),
            ("pure subroutine bad()\nbackspace(20)\nend subroutine bad", "PURE procedure cannot perform external I/O"),
        ]:
            result = subprocess.run([COMPILER, self.source(source)], capture_output=True, text=True, timeout=60)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(error, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
