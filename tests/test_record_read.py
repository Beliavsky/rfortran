"""Formatted input checks on Windows, using the system LLD linker.

Run after cargo build --bin f90c: python tests/test_record_read.py
Set F90C_TEST_COMPILER to test another build (e.g. target/release/f90c.exe).
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
COMPILER = Path(os.environ.get("F90C_TEST_COMPILER", ROOT / "target/debug/f90c.exe")).resolve()


@unittest.skipUnless(os.name == "nt", "Windows native linker configuration")
class RecordRead(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="record-read-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        cls.source = ROOT / "tests/io/record_read.f90"
        batch = (ROOT / "f90c.bat").read_text()
        libs = [ROOT / "src/libs/x64"] + [Path(re.search(r'set "' + name + r'=(.*?)"', batch).group(1))
                 for name in ("MSVC_LIB", "UCRT_LIB")]
        cls.commands = [[COMPILER, "run", cls.source]]
        for opt in (0, 2):
            obj = cls.directory / f"read{opt}.obj"
            exe = obj.with_suffix(".exe")
            for cmd in ([COMPILER, "-O", str(opt), "emit-obj", cls.source, obj],
                        [shutil.which("ld.lld.exe"), "-flavor", "link", "/OUT:" + str(exe), obj,
                         "/SUBSYSTEM:CONSOLE", *("/LIBPATH:" + str(p) for p in libs),
                         "msvcrt.lib", "legacy_stdio_definitions.lib", "kernel32.lib"]):
                result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
                if result.returncode:
                    raise AssertionError(result.stdout + result.stderr)
            cls.commands.append([exe])

    def run_input(self, data, expected=None, error=None):
        for command in self.commands:
            with self.subTest(command=command):
                result = subprocess.run(command, input=data, capture_output=True, timeout=15)
                if error:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error.encode(), result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, b"")
                    self.assertEqual(result.stdout.replace(b"\r\n", b"\n"), expected)

    def test_records_padding_truncation_and_final_record_without_newline(self):
        data = b"  abc  \n\n123456789012\n42 unused record contents\n" + b"x" * 12000 + b"\nignored\nZebra"
        expected = b"[  abc   ]\n[        ]\n[12345678]\n42\n[" + b"x" * 10000 + b"]\n[]\n[Z]\n"
        self.run_input(data, expected)
        self.run_input(data.replace(b"\n", b"\r\n"), expected)
        if shutil.which("gfortran"):
            exe = self.directory / "reference.exe"
            result = subprocess.run(["gfortran", self.source, "-o", exe], cwd=self.directory,
                                    capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], input=data, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.replace(b"\r\n", b"\n"), expected)

    def test_eof_is_an_error(self):
        self.run_input(b"", error="end of file or input error")
        self.run_input(b"one\ntwo\nthree\n42\nlong\n", error="end of file or input error")

    def test_unsupported_bytes_are_rejected_even_beyond_destination(self):
        for data in (b"\0\n", b"a" * 100 + b"\0\n", b"\xc3\xa9\n"):
            self.run_input(data, error="non-NUL ASCII input")


if __name__ == "__main__":
    unittest.main(verbosity=2)
