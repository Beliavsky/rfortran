"""The warning examples in warnings/: each program warns exactly where it says.

Each warnings/<category>.f90 is a complete program with a header naming its
category and the option group that enables it, and `! expect: <category>`
on each line that should be reported. Other lines show code that is
deliberately not reported. This checks, for each file:

- `rfort -W<category> check` reports that category on exactly the marked
  lines;
- the group in the header reports the same lines, and a narrower setting
  reports none (no option for a -Wall warning; -Wall, -Wextra and -Wstyle
  together for one enabled only by -Weverything or its own option);
- -Weverything reports the same lines, unless the header says the category
  is enabled by its own option only;
- gfortran, when installed, accepts the program.

Run after cargo build --bin rfort: python tests/test_warning_examples.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_record_read import ROOT, COMPILER

EXAMPLES = ROOT / "warnings"
GROUPS = ("-Wall", "-Wextra", "-Wstyle")
EVERYTHING = "-Weverything"


def header(path):
    """The category and the settings that enable it, from the header."""
    text = path.read_text(encoding="utf-8")
    category = re.search(r"^! Warning:\s*(\S+)", text, re.M)
    enabled = re.search(r"^! Enabled by:\s*(.+)$", text, re.M)
    if not category or not enabled:
        raise AssertionError(f"{path.name} needs '! Warning:' and '! Enabled by:' header lines")
    return category.group(1), [e.strip() for e in enabled.group(1).split(",")]


def expected_lines(path, category):
    """The lines marked `! expect: <category>`, and the lines after a
    comment line `! expect-next: <category>`, for code that a comment of its
    own would change."""
    lines = set()
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        marker = re.search(r"!\s*expect(-next)?:\s*([\w\-, ]+)", line)
        if marker and category in [c.strip() for c in marker.group(2).split(",")]:
            lines.add(number + 1 if marker.group(1) else number)
    return lines


def reported_lines(path, category, options):
    """The lines rfort reports `category` on, with these options."""
    result = subprocess.run([str(COMPILER), *options, "check", path.name], cwd=path.parent,
                            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
    output = result.stdout + result.stderr
    lines, pending = set(), False
    for line in output.splitlines():
        if re.match(r"(warning|error): .*\[" + re.escape(category) + r"\]\s*$", line):
            pending = True
        elif pending:
            location = re.search(re.escape(path.name) + r":(\d+):\d+", line)
            if location:
                lines.add(int(location.group(1)))
                pending = False
    return lines, output


class WarningExamples(unittest.TestCase):
    def test_examples(self):
        paths = sorted(EXAMPLES.glob("*.f90"))
        self.assertTrue(paths, "no examples in warnings/")
        for path in paths:
            with self.subTest(example=path.name):
                category, enabled = header(path)
                self.assertEqual(path.stem, category, "the file is named after its category")
                expected = expected_lines(path, category)
                self.assertTrue(expected, "no '! expect:' lines")
                own, output = reported_lines(path, category, [f"-W{category}"])
                self.assertEqual(own, expected, output)
                for setting in enabled:
                    if setting == "default":
                        options = []
                    elif setting in GROUPS or setting == EVERYTHING:
                        options = [setting]
                    else:
                        self.assertEqual(setting, f"-W{category} only", f"unknown setting {setting!r}")
                        continue
                    got, output = reported_lines(path, category, options)
                    self.assertEqual(got, expected, f"with {options or 'no options'}:\n{output}")
                # -Weverything reports every category but those enabled by
                # their own option only.
                only = f"-W{category} only" in enabled
                got, output = reported_lines(path, category, [EVERYTHING])
                self.assertEqual(got, set() if only else expected, f"with {EVERYTHING}:\n{output}")
                if "default" not in enabled:
                    # Neither no options nor the other groups report it.
                    others = [g for g in GROUPS if g not in enabled]
                    for options in ([], others) if others else ([],):
                        got, output = reported_lines(path, category, options)
                        self.assertEqual(got, set(), f"with {options or 'no options'}:\n{output}")

    def test_readme_and_usage_link_every_example(self):
        readme = (EXAMPLES / "README.md").read_text(encoding="utf-8")
        usage = (ROOT / "docs" / "usage.md").read_text(encoding="utf-8")
        for path in sorted(EXAMPLES.glob("*.f90")):
            self.assertIn(f"]({path.name})", readme, f"{path.name} is missing from warnings/README.md")
            self.assertIn(f"](../warnings/{path.name})", usage, f"docs/usage.md does not link {path.name}")

    @unittest.skipUnless(shutil.which("gfortran"), "gfortran is not installed")
    def test_gfortran_accepts_examples(self):
        with tempfile.TemporaryDirectory() as work:
            for path in sorted(EXAMPLES.glob("*.f90")):
                # A header line "! gfortran: no (reason)" marks a program
                # that uses a feature gfortran does not support yet.
                if re.search(r"^! gfortran:\s*no\b", path.read_text(encoding="utf-8"), re.M):
                    continue
                with self.subTest(example=path.name):
                    result = subprocess.run(["gfortran", "-fsyntax-only", str(path)], cwd=work,
                                            capture_output=True, text=True, timeout=120)
                    self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
