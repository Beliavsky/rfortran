"""Scalar substring bounds and bounded deferred-length CHARACTER assignment."""
import json
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
import test_character_comparisons


class ScalarSubstrings(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def test_deferred_length_literal_substring(self):
        source = ROOT / "tests/119_dynamic_substrings.f90"
        with tempfile.TemporaryDirectory(prefix="deferred-substring-") as tmp:
            self.compare(source, '23456\n5 T\n[x]\n0 T\n', Path(tmp))

    def test_runtime_bounds_resize_and_overlap(self):
        program = '''implicit none
character(:),allocatable::s,t
character(10)::full
integer::first,last
full='0123456789'
read(*,*) first
read(*,*) last
s=full(first:last)
t=s
print '(a,a,a,i0)', '[',s,']',len(s)
s=full
s=s(2:4)
print '(a,a,a,i0)', '[',s,']',len(s)
s='x'//full(first:first+1)
print '(a,a,a,i0)', '[',s,']',len(s)
print '(a,a,a,i0)', '[',t,']',len(t)
full='ab  '
s=trim(full)
print '(a,a,a,i0)', '[',s,']',len(s)
print *,len_trim(t),len(full(first:last))
end
'''
        with tempfile.TemporaryDirectory(prefix="runtime-substring-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program)
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                reference = directory / "reference.exe"
                result = subprocess.run(["gfortran", source, "-o", reference], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([reference])
            for command in commands:
                result = subprocess.run(command, input="3\n7\n", capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assertEqual(result.stdout.splitlines()[:5], ['[23456]5','[123]3','[x23]3','[23456]5','[ab]2'])
                self.assertEqual(result.stdout.splitlines()[5].split(), ['5','5'])

    def test_dynamic_loop_conditions_and_empty_substrings(self):
        program = '''implicit none
character(:),allocatable::s
integer::i
s='aabb'
i=1
do while (s(i:i)=='a')
  i=i+1
  cycle
end do
print *,i
s=s(i:)
print *,s=='bb',len(s)
s=s(3:2)
print *,s=='',len(s)
print *,len('abc'(4:3))
end
'''
        with tempfile.TemporaryDirectory(prefix="substring-loop-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program)
            self.compare(source, '3\nT 2\nT 0\n0\n', directory)

    def test_allocation_status_and_kind_bounds(self):
        program = '''implicit none
character(:),allocatable::s
integer(1)::first
integer(2)::last
print *,allocated(s)
first=2
last=3
s='abcd'(first:last)
print *,allocated(s),len(s),s=='bc'
end
'''
        with tempfile.TemporaryDirectory(prefix="substring-allocation-") as tmp:
            directory = Path(tmp)
            source = directory / 'main.f90'
            source.write_text(program)
            self.compare(source, 'F\nT 2 T\n', directory)

    def test_runtime_invalid_bounds_are_checked_before_read(self):
        program = '''implicit none
character(:),allocatable::s
integer::first,last
read(*,*) first
read(*,*) last
s='abcd'(first:last)
print *,s
end
'''
        with tempfile.TemporaryDirectory(prefix="substring-bounds-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text(program)
            for command in build_commands(source, directory):
                for data in ['0\n2\n','1\n5\n','6\n4\n','1\n-1\n']:
                    result = subprocess.run(command, input=data, capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('substring bounds are outside CHARACTER length', result.stderr)
                    self.assertEqual(result.stdout, '')

    def test_deferred_length_diagnostics(self):
        cases = [
            ('character(:)::s', "s='x'", 'requires ALLOCATABLE'),
            ('character(:),allocatable::s(:)', '', 'deferred CHARACTER arrays'),
            ('character(:),allocatable::s', "s='x'\ns=s//'y'", 'unbounded storage growth'),
            ('character(:),allocatable::s', 'print *,s', 'uninitialized variable `s`'),
            ('character(:),allocatable::s\ninteger::i', "s='abcd'(i:3)", 'uninitialized variable `i`'),
            ('character(:),allocatable::s\nreal::i', "i=1.0\ns='abcd'(i:3)", 'scalar INTEGER'),
            ('character(:),allocatable::s', "s=' a '\nprint *,adjustl(s)", 'runtime-length CHARACTER arguments'),
        ]
        with tempfile.TemporaryDirectory(prefix="deferred-diagnostics-") as tmp:
            directory = Path(tmp)
            for declarations, statements, message in cases:
                source = directory / 'main.f90'
                source.write_text(f'implicit none\n{declarations}\n{statements}\nend\n')
                result = subprocess.run([COMPILER, '-c', source, '-o', directory / 'invalid.obj'], capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0, statements)
                self.assertIn(message, result.stderr)

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
            ("s='abcd'\ni=2\ns(i:3)='x'", "bounds must currently be constant"),
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
