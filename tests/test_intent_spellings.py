"""Equivalent INOUT spellings and argument checks in interpreter/native O0/O2."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_character_comparisons
from test_record_read import ROOT, COMPILER


class IntentSpellings(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def test_updates_and_pure_procedures(self):
        source = ROOT / "tests/121_intent_in_out.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="intent-spellings-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_argument_validation_is_equivalent(self):
        with tempfile.TemporaryDirectory(prefix="bad-inout-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            for argument in ("1", "x"):
                messages = []
                for spelling in ("inout", "in out"):
                    with self.subTest(argument=argument, spelling=spelling):
                        source.write_text("program main\nimplicit none\ninteger :: x\n"
                                          f"call bump({argument})\ncontains\nsubroutine bump(n)\n"
                                          f"integer, intent({spelling}) :: n\nn=n+1\n"
                                          "end subroutine bump\nend program main\n", encoding="utf-8")
                        result = subprocess.run([COMPILER, "emit-obj", source, directory / "bad.obj"],
                                                capture_output=True, text=True, timeout=30)
                        self.assertNotEqual(result.returncode, 0, result.stdout)
                        messages.append(result.stderr)
                self.assertEqual(messages[0], messages[1])


if __name__ == "__main__":
    unittest.main()
