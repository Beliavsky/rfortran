"""Constant CHARACTER lengths and assignments: interpreter, native O0/O2, gfortran."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands

class CharacterLengths(unittest.TestCase):
    def check_program(self, source, expected):
        with tempfile.TemporaryDirectory(prefix="character-lengths-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text(source, encoding="utf-8")
            commands = build_commands(path, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                build = subprocess.run(["gfortran", path, "-o", exe], cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(build.returncode, 0, build.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual([line.lstrip() for line in result.stdout.splitlines()], expected.splitlines())

    def test_lengths(self):
        self.check_program((ROOT / "tests/78_character_lengths.f90").read_text(),
            "10\n[abcdefghij]\n[q  ]\n[]\n[]\n[ab]\n[abc    ]\n3\n[1234]\n9")

    def test_assignment_and_aliasing(self):
        self.check_program("""program main
implicit none
character :: one
character(len=5) :: a
character(len=8) :: b
character(len=0) :: empty
integer :: i
one='xyz'
a='ab'
b=a
print *, '['//one//']['//a//']['//b//']'
a=a
print *, '['//a//']'
do i=1,2
 a=trim(a)//'x'
end do
print *, '['//a//']'
b=trim(a)//trim(b)
print *, '['//b//']'
a=b
print *, '['//a//']'
empty=trim(a)
b=empty//trim('   ')
print *, '['//b//']'
a='  z'
b=trim(a)//'!'
print *, '['//b//']'
end program main
""", "[x][ab   ][ab      ]\n[ab   ]\n[abxx ]\n[abxxab  ]\n[abxxa]\n[        ]\n[  z!    ]")

    def test_invalid_lengths(self):
        cases = [
            ("integer :: n\ncharacter(len=n) :: x", "runtime-dependent lengths"),
            ("character(len=1.5) :: x", "constant INTEGER"),
            ("character(len=2147483647) :: x", "too large"),
            ("character(len=1/0) :: x", "division by zero"),
        ]
        with tempfile.TemporaryDirectory(prefix="bad-character-length-") as tmp:
            path=Path(tmp)/"main.f90"
            for declaration, message in cases:
                with self.subTest(declaration=declaration):
                    path.write_text("program main\n"+declaration+"\nend program main\n")
                    result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_rank_two_character_storage(self):
        self.check_program('''implicit none
character(len=2) :: x(2,2)
x='ab'
x(2,1)='z'
print '(a)', '['//x(2,1)//']'
print *,size(x)
end
''', '[z ]\n4')

    def test_trim_removes_spaces_only(self):
        self.check_program("program main\ncharacter(4) :: a,b\na='x\t '\nb=trim(a)//'!'\nprint *, '['//b//']'\nend program main\n", "[x\t! ]")

    def test_conditional_initialization(self):
        with tempfile.TemporaryDirectory(prefix="conditional-character-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text("program main\ninteger :: flag\ncharacter(4) :: a,b\nread(*,*) flag\nif (flag > 0) then\na='x'\nend if\nb=a\nprint *, '['//b//']'\nend program main\n")
            for command in build_commands(path, directory):
                for value, success in [("1\n", True), ("0\n", False)]:
                    with self.subTest(command=command, value=value):
                        result = subprocess.run(command, input=value, capture_output=True, text=True, timeout=15)
                        if success:
                            self.assertEqual(result.returncode, 0, result.stderr)
                            self.assertEqual(result.stdout.lstrip(), "[x   ]\n")
                        else:
                            self.assertNotEqual(result.returncode, 0)
                            self.assertIn("uninitialized variable `a`", result.stderr)

    def test_uninitialized_source(self):
        with tempfile.TemporaryDirectory(prefix="unset-character-") as tmp:
            path=Path(tmp)/"main.f90"
            path.write_text("program main\ncharacter(len=3) :: a\na=trim(a)//'x'\nprint *, a\nend program main\n")
            result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("uninitialized variable `a`",result.stderr)

if __name__ == "__main__":
    unittest.main(verbosity=2)
