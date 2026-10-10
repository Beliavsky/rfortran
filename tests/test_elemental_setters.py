"""Scalar elemental setters with OPTIONAL/PRESENT, including CHARACTER lengths."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands


class ElementalSetters(unittest.TestCase):
    def test_four_setters_and_reference(self):
        source = ROOT / "tests/86_elemental_setters.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="elemental-setters-") as tmp:
            directory = Path(tmp)
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", source, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, expected)
                    self.assertEqual(result.stderr, "")

    def test_diagnostics(self):
        routine = """elemental subroutine pick(x,default,option)
integer,intent(out)::x
integer,intent(in)::default
integer,intent(in),optional::option
if (present(option)) then
 x=option
else
 x=default
end if
end subroutine pick
"""
        cases = [
            ("integer::x", "call pick(x)", routine, "missing required argument"),
            ("integer::x", "call pick(x,1,2,3)", routine, "too many arguments"),
            ("integer::x", "call pick(x,1.0)", routine, "type mismatch"),
            ("integer::x(2)", "call pick(x,1)", routine, "array actual arguments"),
            ("integer::x(2)", "call pick(x(:),1)", routine, "array actual arguments"),
            ("", "call pick(1,2)", routine, "requires a variable"),
            ("integer::x,y", "call pick(x,y)", routine, "uninitialized variable `y`"),
            ("integer::x", "call pick(x,1)", routine.replace("if (present(option)) then", "if (.true.) then"), "absent OPTIONAL"),
            ("integer::x", "call pick(x,1)", routine.replace("present(option)", "present(default)"), "OPTIONAL dummy"),
            ("integer::x", "call pick(x,1)", routine.replace("integer,intent(out)::x", "integer,intent(out)::x(2)"), "scalar dummy"),
            ("integer::x", "call pick(x,1)", routine.replace(" x=default", " print *,default\n x=default"), "external I/O"),
            ("integer::x", "call pick(x,1)", routine.replace("integer,intent(in)::default", "integer::default"), "explicit INTENT"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-setter-") as tmp:
            source = Path(tmp) / "main.f90"
            for decl, call, definition, message in cases:
                with self.subTest(call=call, message=message):
                    source.write_text("program main\n"+decl+"\n"+call+"\ncontains\n"+definition+"end program main\n")
                    result = subprocess.run([COMPILER, "check", source], capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
