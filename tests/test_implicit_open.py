"""READ and WRITE on an unconnected unit 1..1023 connect it to the file fort.N,
as gfortran and ifx do, and -Wimplicit-open, on by default, warns about such a
constant unit that no OPEN statement names.

Each program runs in its own directory in the interpreter, at -O0 and -O2,
and with gfortran when it is installed; stdout and the fort.N files must match.
Run after cargo build --bin rfort: python tests/test_implicit_open.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

TRANSFERS = '''program main
  implicit none
  logical :: op
  character(len=40) :: nm
  integer :: ios, k
  integer, parameter :: log_unit = 30
  inquire(unit=20, opened=op); print *, 'before', op
  write (20,*) 'first'
  write (20,'(a)') 'second'
  inquire(unit=20, opened=op, name=nm); print *, 'after', op, trim(nm)
  rewind 20
  read (20,'(a)') nm; print *, trim(nm)
  close(20)
  write (20,'(a)') 'reopened'
  close(20)
  read (21,*,iostat=ios) k; print *, 'read missing', ios < 0
  inquire(unit=21, opened=op); print *, '21 opened', op
  write (log_unit,'(a,i0)') 'log ', 7
  print *, 'done'
end program
'''


def normalized(data):
    return data.replace(b'\r\n', b'\n')


class ImplicitOpen(unittest.TestCase):
    def commands(self, directory, source):
        commands = [[COMPILER, 'run', source]]
        for opt in (0, 2):
            exe = directory / f'p{opt}.exe'
            result = subprocess.run([COMPILER, '-O', str(opt), '-q', 'build', source, '-o', exe],
                                    cwd=directory, capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            commands.append([exe])
        if shutil.which('gfortran'):
            exe = directory / 'reference.exe'
            result = subprocess.run(['gfortran', source, '-o', exe], cwd=directory,
                                    capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands.append([exe])
        return commands

    def run_all(self, program):
        """Each command's stdout and fort.N files, run in a fresh directory."""
        with tempfile.TemporaryDirectory(prefix='implicit-open-') as tmp:
            directory = Path(tmp)
            source = directory / 'example.f90'
            source.write_text(program, encoding='utf-8')
            outcomes = []
            for command in self.commands(directory, source):
                for old in directory.glob('fort.*'):
                    old.unlink()
                result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, f'{command}: {result.stderr}')
                files = {f.name: normalized(f.read_bytes()) for f in sorted(directory.glob('fort.*'))}
                outcomes.append((command, result.stdout, files))
            return outcomes

    def test_transfers_match_gfortran(self):
        outcomes = self.run_all(TRANSFERS)
        for command, stdout, files in outcomes:
            with self.subTest(command=command):
                self.assertEqual(stdout.split(), 'before F after T fort.20 first read missing T 21 opened T done'.split())
                self.assertEqual(files, {'fort.20': b'reopened\n', 'fort.21': b'', 'fort.30': b'log 7\n'})

    def check(self, program, flags=()):
        with tempfile.TemporaryDirectory(prefix='implicit-open-warning-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text(program, encoding='utf-8')
            result = subprocess.run([COMPILER, *flags, 'check', source], cwd=tmp,
                                    capture_output=True, text=True, timeout=30)
            return result.returncode, result.stderr

    def test_warning_on_by_default_and_controls(self):
        code, stderr = self.check('write (20,*) "abc"\nwrite (20,*) "def"\nend\n')
        self.assertEqual(code, 0)
        self.assertEqual(stderr.count('[implicit-open]'), 1, stderr)
        self.assertIn('WRITE on unit 20', stderr)
        self.assertIn('fort.20', stderr)
        self.assertIn('example.f90:1:1', stderr)
        for flags in (['-Wno-implicit-open'], ['-Wall', '-Wextra', '-Wno-implicit-open']):
            self.assertEqual(self.check('write (20,*) 1\nend\n', flags), (0, ''))
        self.assertEqual(self.check('!#allow(implicit-open)\nwrite (20,*) 1\nend\n'), (0, ''))
        for flags in (['-Werror'], ['-Werror=implicit-open']):
            code, stderr = self.check('write (20,*) 1\nend\n', flags)
            self.assertNotEqual(code, 0)
            self.assertIn('error: WRITE on unit 20', stderr)
        code, stderr = self.check('write (20,*) 1\nend\n', ['-std=f2023'])
        self.assertIn('[implicit-open]', stderr)

    def test_units_the_warning_skips(self):
        for program in (
            "open(20, file='a.txt')\nwrite(20,*) 1\nend\n",
            "open(file='a.txt', unit=20)\nwrite(unit=20, fmt=*) 1\nend\n",
            "integer, parameter :: u = 20\nopen(u, file='a.txt')\nwrite(20,*) 1\nend\n",
            "write(6,*) 1\nwrite(0,*) 2\nwrite(*,*) 3\nprint *, 4\nend\n",
            "character(8) :: s\nwrite(s,'(i3)') 5\nend\n",
            "integer :: iu\niu = 20\nopen(iu, file='a.txt')\nwrite(21,*) 1\nend\n",
            "integer :: iu\niu = 20\nwrite(iu,*) 1\nend\n",
        ):
            with self.subTest(program=program):
                self.assertEqual(self.check(program), (0, ''))
        code, stderr = self.check("integer :: iu\nopen(newunit=iu, file='a.txt')\nread(21,*) iu\nend\n")
        self.assertIn('READ on unit 21', stderr)


if __name__ == '__main__':
    unittest.main()
