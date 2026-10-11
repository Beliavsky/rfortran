"""Fixed-bound scalar substring assignment and overlapping reads."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER
import test_character_comparisons


class ScalarSubstrings(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def test_overlap(self):
        source = ROOT / "tests/115_scalar_substrings.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="substring-overlap-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_scopes_and_empty_ranges(self):
        program = """program main
character(6)::s,unset
character(0)::z
integer,parameter::first=2,last=4
integer::i
s(:)='abcdef'
z(:)=''
print *,len(unset(2:4)),len(unset(4:2)),len(z(:))
unset='xxxxxx'
print *,unset(1:1)=='x'
print *,s(first:last)=='bcd',s(:)=='abcdef',z(:)==''
print *,s(7:6)=='',s(1:0)==''
i=0
do while(s(1:1)=='a')
  i=i+1
  s(1:1)='z'
  cycle
end do
print *,i,s=='zbcdef'
call change(s)
print *,s=='zCDef '
print *,match(s(2:3)),match(s(3:4))
contains
subroutine change(x)
character(*),intent(inout)::x
if(x(2:3)=='bc') then
  x(2:3)='CD'
end if
x(4:)=x(5:)
end subroutine change
pure integer function match(x)
character(2),intent(in)::x
match=0
if(x(:)=='CD') match=1
end function match
end program
"""
        with tempfile.TemporaryDirectory(prefix="substring-scopes-") as tmp:
            directory = Path(tmp)
            source = directory / "scopes.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "3 0 0\nT\nT T T\nT T\n1 T\nT\n1 0\n", directory)

    def test_diagnostics(self):
        cases = [
            ("s='abcd'\nprint *,s(0:2)", "outside CHARACTER length"),
            ("s='abcd'\ns(2:5)='x'", "outside CHARACTER length"),
            ("s='abcd'\nprint *,s(::2)", "do not allow a stride"),
            ("s='abcd'\ni=2\nprint *,s(i:3)", "bounds must currently be constant"),
            ("s(2:3)='ab'", "uninitialized variable `s`"),
            ("print *,s(2:3)", "uninitialized variable `s`"),
        ]
        with tempfile.TemporaryDirectory(prefix="substring-errors-") as tmp:
            directory = Path(tmp)
            for statements, message in cases:
                with self.subTest(statements=statements):
                    source = directory / "invalid.f90"
                    source.write_text(f"program main\ncharacter(4)::s\ninteger::i\n{statements}\nend\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "invalid.obj"],
                                            capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
