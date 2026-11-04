"""ACTION= of OPEN: a READ from a unit opened with ACTION='WRITE', or a WRITE to
one opened with ACTION='READ', is an error. -Waction-mismatch, on by default,
reports one that holds on every path at compile time; at run time the
transfer stops the program, or sets a positive IOSTAT= on READ.

Each program runs in its own directory in the interpreter, at -O0 and -O2,
and with gfortran when it is installed.
Run after cargo build --bin rfort: python tests/test_action_mismatch.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

BAD_READ = '''implicit none
integer, parameter :: iu = 20
integer :: i
open (unit=iu, file="temp.txt", action="write")
read (iu,*) i
print*,"i =",i
end
'''

BAD_WRITE = '''implicit none
integer, parameter :: iu = 20
open (unit=iu, file="temp.txt", action="read")
write (iu,*) 234
end
'''

STATUS = '''program p
implicit none
integer :: u, ios, i
character(len=20) :: s
open(newunit=u, file="data.txt", action="write")
write(u,*) 7
read(u,*,iostat=ios) i
print '(l1)', ios > 0
read(u,'(a)',iostat=ios) s
print '(l1)', ios > 0
close(u)
open(newunit=u, file="data.txt", action="read")
read(u,*) i
print '(i0)', i
close(u)
open(newunit=u, file="data.bin", access="stream", form="unformatted", action="write")
write(u) 3
read(u,iostat=ios) i
print '(l1)', ios > 0
close(u, status="delete")
end program
'''

# The analysis reports only an action that is certain on every path.
UNCERTAIN = '''program p
implicit none
integer :: i, ios
logical :: c
c = .false.
open (20, file="a.txt", action="write")
read (20,*,iostat=ios) i
close (20)
open (20, file="a.txt", action="read")
read (20,*) i
close (20)
if (c) then
  open (21, file="b.txt", action="write")
else
  open (21, file="b.txt", action="read")
end if
if (c) read (21,*) i
close (21)
end program
'''

LOOP = '''program p
implicit none
integer :: k
open (22, file="c.txt", action="read")
do k = 1, 3
  write (22,*) k
end do
end program
'''


class ActionMismatch(unittest.TestCase):
    def compile(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='action-') as tmp:
            path = Path(tmp) / 'example.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], capture_output=True, text=True, timeout=120)

    def commands(self, directory, source):
        commands = [[COMPILER, 'run', source]]
        for opt in (0, 2):
            exe = directory / f'p{opt}.exe'
            result = subprocess.run([COMPILER, '-O', str(opt), '-q', '-Wno-action-mismatch', 'build', source, '-o', exe],
                                    cwd=directory, capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            commands.append([exe])
        return commands

    def run_all(self, program, reference=True):
        """Each command's exit status, stdout and stderr, in a fresh directory."""
        with tempfile.TemporaryDirectory(prefix='action-') as tmp:
            directory = Path(tmp)
            source = directory / 'example.f90'
            source.write_text(program, encoding='utf-8')
            commands = self.commands(directory, source)
            if reference and shutil.which('gfortran'):
                exe = directory / 'reference.exe'
                result = subprocess.run(['gfortran', source, '-o', exe], cwd=directory,
                                        capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            outcomes = []
            for command in commands:
                (directory / 'temp.txt').write_text('345\n', encoding='utf-8')
                result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=15)
                outcomes.append((command, result.returncode, result.stdout, result.stderr))
            return outcomes

    def test_warnings(self):
        result = self.compile(BAD_READ)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("READ from unit 20, which is opened with ACTION='WRITE', is an error at run time [action-mismatch]", result.stderr)
        self.assertIn("opened with ACTION='WRITE' here", result.stderr)
        result = self.compile(BAD_WRITE)
        self.assertIn("WRITE to unit 20, which is opened with ACTION='READ', is an error at run time [action-mismatch]", result.stderr)
        result = self.compile(LOOP)
        self.assertIn("WRITE to unit 22", result.stderr)
        self.assertNotIn('action-mismatch', self.compile(BAD_READ, '-Wno-action-mismatch').stderr)
        result = self.compile(BAD_WRITE, '-Werror=action-mismatch')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error', result.stderr)

    def test_uncertain_actions_are_not_reported(self):
        for source in (UNCERTAIN, STATUS):
            result = self.compile(source)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn('action-mismatch', result.stderr)

    def test_transfers_stop(self):
        for program, message in ((BAD_READ, "READ from a unit opened with ACTION='WRITE'"),
                                 (BAD_WRITE, "WRITE to a unit opened with ACTION='READ'")):
            for command, code, stdout, stderr in self.run_all(program, reference=False):
                self.assertNotEqual(code, 0, command)
                self.assertEqual(stdout, '', command)
                self.assertIn(message, stderr, command)

    def test_iostat_reports_the_error(self):
        for command, code, stdout, stderr in self.run_all(STATUS):
            self.assertEqual(code, 0, f'{command}: {stderr}')
            self.assertEqual(stdout.split(), ['T', 'T', '7', 'T'], command)


if __name__ == '__main__':
    unittest.main()
