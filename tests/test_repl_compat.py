"""Shared ofort/rfortran transcripts plus rfortran-specific REPL contracts.

Set RFORT_TEST_COMPILER and optionally OFORT_TEST_INTERPRETER. ofort runs in
its native --autorun mode; normal input is incremental in rfortran. These
fixtures check common results/source, allowing native banner/format differences.
No files in the ofort project are modified.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

CASES = json.loads(Path(__file__).with_name("repl_compat.json").read_text())


class ReplCompatibility(unittest.TestCase):
    def session(self, source):
        result = subprocess.run([COMPILER, "repl"], input=source, text=True,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def shared(self, ofort=False):
        interpreter = os.environ.get("OFORT_TEST_INTERPRETER")
        if ofort and not interpreter:
            self.skipTest("set OFORT_TEST_INTERPRETER to also run the same fixtures with ofort")
        for case in CASES:
            with self.subTest(case=case["name"]), tempfile.TemporaryDirectory(prefix="repl-compat-") as d:
                directory = Path(d)
                if ofort:
                    empty = directory / "empty.f90"
                    empty.write_text("")
                    command = [interpreter, "--nologo", "-w", "--prompt", "", "--autorun", "--load", str(empty)]
                else:
                    command = [COMPILER, "repl"]
                result = subprocess.run(command, input=case["input"] + ".save saved.f90\n.quit!\n",
                                        text=True, capture_output=True, timeout=30, cwd=d)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                stdout = " ".join(result.stdout.split())
                for expected in case["stdout_contains"]:
                    self.assertIn(expected, stdout)
                source = (directory / "saved.f90").read_text()
                canonical = lambda s: re.sub(r"\s+", "", s).lower()
                for expected in case.get("source_contains", []):
                    self.assertIn(canonical(expected), canonical(source))
                for excluded in case.get("source_excludes", []):
                    self.assertNotIn(canonical(excluded), canonical(source))
                if compiler := shutil.which("gfortran"):
                    checked = subprocess.run([compiler, "-fsyntax-only", "saved.f90"], cwd=d,
                                             text=True, capture_output=True, timeout=15)
                    self.assertEqual(checked.returncode, 0, checked.stderr + source)

    def test_shared_rfortran_transcripts(self):
        self.shared()

    def test_shared_ofort_transcripts(self):
        self.shared(ofort=True)

    def test_inspection_of_unset_and_unallocated_arrays(self):
        result = self.session("""integer :: n, a(-1:0,2:3)
a(-1,3)=7
real, allocatable :: x(:)
.vars
.shapes a
.sizes a
.quit
""")
        self.assertEqual(result.stderr, "")
        self.assertEqual(result.stdout, "a = [<unset>, <unset>, 7, <unset>]\nn = <unset>\nx = <unallocated>\na = [2, 2]\na = 4\n")

    def test_large_array_preview_and_constant_values(self):
        result = self.session("""integer :: a(8)
a=[1,2,3,4,5,6,7,8]
const n=3, yes=.true., s='hi'
.vars
.info
.quit
""")
        self.assertEqual(result.stderr, "")
        self.assertIn("a = [1, 2, 3, ..., 6, 7, 8]", result.stdout)
        for line in ("n = 3", "yes = T", "s = 'hi'"):
            self.assertIn(line, result.stdout)
        self.assertNotIn("<unset>", result.stdout)

    def test_inspection_does_not_advance_random_state_or_read_unset_values(self):
        result = self.session("""real :: x,y
call random_number(x)
.vars
.info
.shapes
.list -n
call random_number(y)
print *,y
.quit
""")
        self.assertEqual(result.stderr, "")
        self.assertIn("y = <unset>", result.stdout)
        self.assertTrue(result.stdout.endswith(" 0.671137\n"))

    def test_shortcuts_are_first_definitions_and_failed_lines_roll_back(self):
        result = self.session("""let n=3
let n=7
const k=2
const k=9
k=4
let partial=1, bad=unknown
.info
.vars n k
.quit
""")
        self.assertIn("already declared", result.stderr)
        self.assertIn("cannot modify PARAMETER", result.stderr)
        self.assertIn("unknown", result.stderr)
        self.assertNotIn("partial", result.stdout)
        self.assertTrue(result.stdout.endswith("k = 2\nn = 3\n"))

    def test_dot_literals_and_variables_named_like_shortcuts(self):
        result = self.session("""integer :: let=2
logical :: const=.true.
let
const
.true.
.5
.not. .false.
.quit!
""")
        self.assertEqual(result.stderr, "")
        self.assertEqual(result.stdout, " 2\n T\n T\n 0.5\n T\n")

    def test_let_array_and_inspection_errors(self):
        result = self.session("""let a=[1.,2.]
a=[a,3.]
.vars a
.sizes a
.vars missing
.shapes missing
let n=3
.shapes n
.quit
""")
        self.assertIn("unknown REPL variable", result.stderr)
        self.assertIn("not an array", result.stderr)
        self.assertEqual(result.stdout, "a = [1, 2, 3]\na = 3\n")

    def test_numbered_listing_and_aliases(self):
        result = self.session("""let n=2
.list
.clear
:vars
:q
""")
        self.assertEqual(result.stderr, "")
        self.assertRegex(result.stdout, r"(?m)^\s*1 ! Exported")
        self.assertIn("integer :: n", result.stdout)
        self.assertIn("n = 2", result.stdout)


if __name__ == "__main__":
    unittest.main()
