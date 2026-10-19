"""Fixed rank-one CHARACTER storage and overlapping array/substring assignment."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
import test_character_comparisons


class CharacterArrays(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def test_formatted_character_constructors(self):
        cases = [
            ('implicit none\ncharacter(len=3) :: s="abc"\nprint "(a)", [s]\nend\n', "abc\n"),
            ('''implicit none
character(3) :: s="abc", t="def", a(2)
a=["abc","def"]
print "(*(a))", [s,t]
print "(*(a))", [[s],[t]]
print "(*(a))", [a,s]
print "(*(a))", [character(4) :: "abc","xy"]
print "(*(a))", [character(2) :: "abc","def"]
print "(*(a))", [character(3) ::]
end
''', "abcdef\nabcdef\nabcdefabc\nabc xy  \nabde\n\n"),
            ('''implicit none
character(3) :: s="abc",t="def"
character(6) :: fmt="(2a3)"
print fmt,[s,t]
write(*,"(2a3)",advance="no") [s,t]
print "(a)","!"
end
''', "abcdef\nabcdef!\n"),
        ]
        with tempfile.TemporaryDirectory(prefix="character-constructor-output-") as tmp:
            directory=Path(tmp)
            for index,(program,expected) in enumerate(cases):
                with self.subTest(index=index):
                    source=directory / f"constructor{index}.f90"
                    source.write_text(program,encoding="utf-8")
                    self.compare(source,expected,directory)

    def test_typed_character_constructor_runtime_elements(self):
        program='''implicit none
character(6) :: s="abcdef"
character(2) :: t="xy",a(2)
character(4) :: b(3)
a=["pq","rs"]
print "(a)", [character(len=3) :: s,"def"]
print "(*(a))", "[",[character(4) :: s,t],"]"
print "(*(a))", "[",[character(2) :: s,t],"]"
print "(*(a))", "[",[character(0) :: s,t],"]"
print "(*(a))", "[",[character(3) :: a,s],"]"
b=[character(4) :: s,t,"zzz"]
print "(*(a))", "[",b,"]"
print *,len([character(4) :: s,t]),size([character(4) :: s,t])
end
'''
        with tempfile.TemporaryDirectory(prefix="typed-character-runtime-") as tmp:
            directory=Path(tmp)
            source=directory / "typed.f90"
            source.write_text(program,encoding="utf-8")
            self.compare(source,"abc\ndef\n[abcdxy  ]\n[abxy]\n[]\n[pq rs abc]\n[abcdxy  zzz ]\n4 2\n",directory)

    def test_typed_character_constructor_calls_are_evaluated_once(self):
        program='''module m
integer :: calls=0
contains
function next_text() result(s)
character(2) :: s
calls=calls+1
s="ab"
end function
end module
program main
use m
implicit none
print "(*(a))", "[",[character(3) :: next_text(),next_text()],"]"
print *,calls
end program
'''
        with tempfile.TemporaryDirectory(prefix="typed-character-calls-") as tmp:
            directory=Path(tmp)
            source=directory / "calls.f90"
            source.write_text(program,encoding="utf-8")
            self.compare(source,"[ab ab ]\n2\n",directory)

    def test_overlap(self):
        source = ROOT / "tests/116_character_arrays.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="character-arrays-") as tmp:
            self.compare(source, expected, Path(tmp))

    def test_bounds_and_branches(self):
        program = """program main
character(3)::a(-1:1)
character(2)::initial(2)=['ab','cd']
integer::i
print *,all(initial==['ab','cd'])
a=['abc','def','ghi']
a=a(1:-1:-1)
print *,all(a==['ghi','def','abc'])
a=a(1:-1:-1)
if(a(-1)=='abc') then
  a(0:1)=a(-1:0)
  print *,all(a=='zzz')
end if
print *,all(a==['abc','abc','def'])
i=0
do while(a(-1)=='abc')
  a(-1)='xyz'
  i=i+1
  cycle
end do
print *,i,a(-1)=='xyz'
a(-1:1)(2:3)=a(-1:1)(1:2)
print *,all(a==['xxy','aab','dde'])
print *,lbound(a,1),ubound(a,1)
a=a(1)(2:2)//a(-1)(1:1)
print *,all(a=='dx ')
print *,all(a(::-1)=='x')
end program
"""
        with tempfile.TemporaryDirectory(prefix="character-array-bounds-") as tmp:
            directory = Path(tmp)
            source = directory / "bounds.f90"
            source.write_text(program, encoding="utf-8")
            self.compare(source, "T\nT\nF\nT\n1 T\nT\n-1 1\nT\nT\n", directory)

    def test_diagnostics(self):
        cases = [
            ("a='ab'\nprint *,all(a)", "requires a LOGICAL array mask"),
            ("a(1)='ab'\nprint *,a", "variable `a`: element 2"),
            ("a=['a ','bb','cc']", "nonconforming CHARACTER array assignment"),
            ("a=['a','bb']", "constructor elements must have equal lengths"),
            ("a='ab'\nprint *,a(3)", "outside 1:2"),
        ]
        with tempfile.TemporaryDirectory(prefix="character-array-errors-") as tmp:
            directory = Path(tmp)
            for statements, message in cases:
                with self.subTest(statements=statements):
                    source = directory / "invalid.f90"
                    source.write_text(f"program main\ncharacter(2)::a(2)\ninteger::i\n{statements}\nend\n", encoding="utf-8")
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "invalid.obj"],
                                            capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)

    def test_runtime_initialization(self):
        program = """program main
character(2)::a(2)
integer::i
read(*,*)i
if(i==1) a(1)='ok'
a(2)='ok'
print *,all(a=='ok')
end program
"""
        with tempfile.TemporaryDirectory(prefix="character-array-init-") as tmp:
            directory = Path(tmp)
            source = directory / "init.f90"
            source.write_text(program, encoding="utf-8")
            for command in build_commands(source, directory):
                with self.subTest(command=command):
                    ok = subprocess.run(command, input="1\n", capture_output=True, text=True, timeout=15)
                    self.assertEqual(ok.returncode, 0, ok.stderr)
                    self.assertIn("T", ok.stdout.split())
                    bad = subprocess.run(command, input="0\n", capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(bad.returncode, 0)
                    self.assertIn("variable `a`: element 1", bad.stderr)


if __name__ == "__main__":
    unittest.main()
