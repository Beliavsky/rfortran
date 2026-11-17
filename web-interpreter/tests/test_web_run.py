"""rfort_web_run.py runs the playground's WebAssembly interpreter locally.

Uses the built runtime/rfort-browser.wasm (python web-interpreter/build.py)
and the wasmtime package; skips without them unless RFORT_WASM_REQUIRED is
set. Nothing is downloaded.
"""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parents[1]
WASM = HERE / "runtime" / "rfort-browser.wasm"
AVAILABLE = WASM.is_file() and importlib.util.find_spec("wasmtime") is not None
if os.environ.get("RFORT_WASM_REQUIRED") and not AVAILABLE:
    raise RuntimeError("Build the browser interpreter and pip install wasmtime first.")

PROGRAM = """program demo
implicit none
integer :: n, unused_count
real :: x
read *, n
x = n
print '(f0.1)', x*x
end program demo
"""


@unittest.skipUnless(AVAILABLE, "needs runtime/rfort-browser.wasm and the wasmtime package")
class WebRun(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work = tempfile.TemporaryDirectory()
        cls.dir = Path(cls.work.name)
        (cls.dir / "demo.f90").write_text(PROGRAM, encoding="utf-8")
        (cls.dir / "loop.f90").write_text("program loop\ninteger :: i\ni = 0\ndo\n   i = i + 1\nend do\nend program loop\n",
                                          encoding="utf-8")
        (cls.dir / "m.f90").write_text("module m\nimplicit none\ninteger, parameter :: k = 4\nend module m\n", encoding="utf-8")
        (cls.dir / "main.f90").write_text("program main\nuse m\nimplicit none\nprint '(i0)', k*k\nstop 3\nend program main\n",
                                          encoding="utf-8")

    @classmethod
    def tearDownClass(cls):
        cls.work.cleanup()

    def run_script(self, *args, stdin=""):
        # Keep the compiled module out of the user's cache.
        env = {**os.environ, "LOCALAPPDATA": str(self.dir / "cache"), "XDG_CACHE_HOME": str(self.dir / "cache")}
        return subprocess.run([sys.executable, str(HERE / "rfort_web_run.py"), "--wasm", str(WASM), *args],
                              cwd=self.dir, input=stdin, capture_output=True, text=True, encoding="utf-8",
                              errors="replace", env=env, timeout=300)

    def test_run_input_and_warning_options(self):
        plain = self.run_script("demo.f90", "--stdin", "-", stdin="3\n")
        self.assertEqual((plain.returncode, plain.stdout), (0, "9.0\n"), plain.stderr)
        self.assertNotIn("warning", plain.stderr)
        warned = self.run_script("-Wall", "-Wstyle", "demo.f90", "--stdin", "-", stdin="3\n")
        self.assertEqual(warned.stdout, "9.0\n", warned.stderr)
        self.assertIn("`unused_count` is never used [unused-variable]", warned.stderr)
        self.assertIn("[default-real]", warned.stderr)
        self.assertIn("demo.f90:3:", warned.stderr)

    def test_everything(self):
        (self.dir / "quotes.f90").write_text("program q\nimplicit none\nprint *, 'single'\nend program q\n", encoding="utf-8")
        result = self.run_script("-Weverything", "--check", "quotes.f90")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("[quote-style]", result.stderr)

    def test_check_does_not_run(self):
        result = self.run_script("--check", "-Wall", "demo.f90")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "No errors found. The program was checked, not run.\n")
        self.assertIn("unused-variable", result.stderr)

    def test_sources_join_and_status_is_the_programs(self):
        result = self.run_script("m.f90", "main.f90")
        self.assertEqual(result.stdout, "16\n", result.stderr)
        self.assertEqual(result.returncode, 3)

    def test_timeout(self):
        result = self.run_script("--timeout", "2", "loop.f90")
        self.assertEqual(result.returncode, 1)
        self.assertIn("Stopped after 2 seconds.", result.stderr)


if __name__ == "__main__":
    unittest.main()
