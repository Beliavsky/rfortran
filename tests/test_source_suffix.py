"""Free-form sources named .f95, .f03, .f08, .f18 and .f23: they compile like
.f90 files, and -Wsource-suffix (in -Wstyle) suggests .f90."""
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_record_read import COMPILER

PROGRAM = "program p\nimplicit none\nprint *, 'ok'\nend program p\n"


class SourceSuffix(unittest.TestCase):
    def rfort(self, directory, *args):
        return subprocess.run([str(COMPILER), *map(str, args)], cwd=directory, capture_output=True,
                              text=True, encoding="utf-8", errors="replace", timeout=120)

    def test_suffixes_compile_as_free_form(self):
        with tempfile.TemporaryDirectory(prefix="suffix-") as tmp:
            d = Path(tmp)
            for suffix in ("f95", "f03", "f08", "f18", "f23", "F18"):
                with self.subTest(suffix=suffix):
                    source = d / f"p.{suffix}"
                    source.write_text(PROGRAM)
                    for command in (["run", source], ["build", source, "-o", d / "p.exe"]):
                        result = self.rfort(d, *command)
                        self.assertEqual(result.returncode, 0, result.stderr)
                    result = subprocess.run([str(d / "p.exe")], capture_output=True, text=True, timeout=30)
                    self.assertEqual(result.stdout.strip(), "ok")

    def test_warning(self):
        with tempfile.TemporaryDirectory(prefix="suffix-") as tmp:
            d = Path(tmp)
            for name in ("p.f08", "q.f23", "r.F03", "s.f90", "t.F90", "u.f"):
                (d / name).write_text(PROGRAM if name != "u.f" else "      print *, 'ok'\n      end\n")
            warned = lambda name, *options: "[source-suffix]" in self.rfort(d, *options, "check", name).stderr
            out = self.rfort(d, "-Wstyle", "check", "p.f08").stderr
            self.assertIn("suffix `.f08` does not select a Fortran standard", out)
            self.assertIn("rename the file to `p.f90`, and use -std=f2008", out)
            out = self.rfort(d, "-Wsource-suffix", "check", "q.f23").stderr
            self.assertIn("gfortran does not recognize `.f23`", out)
            self.assertIn("rename the file to `r.F90`", self.rfort(d, "-Weverything", "check", "r.F03").stderr)
            for name in ("s.f90", "t.F90", "u.f"):
                self.assertFalse(warned(name, "-Weverything"), name)
            # Opt-in: not by default, -Wall or -Wextra; -Wno- turns it off.
            for options in ([], ["-Wall"], ["-Wextra"], ["-Wstyle", "-Wno-source-suffix"]):
                self.assertFalse(warned("p.f08", *options), options)
            result = self.rfort(d, "-Werror=source-suffix", "check", "p.f08")
            self.assertNotEqual(result.returncode, 0)
            # -Weverything implies -Wall; repeated switches are accepted.
            for options in (["--Wall", "--Wextra", "--Weverything"], ["-Wall", "-Wall", "--Werror", "-Werror"]):
                result = self.rfort(d, *options, "check", "s.f90")
                self.assertEqual(result.returncode, 0, (options, result.stderr))
            self.assertIn("rename the file to `p.f90`", self.rfort(d, "--Wall", "-Weverything", "check", "p.f08").stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
