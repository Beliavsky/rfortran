"""SHIFTL, SHIFTR, SHIFTA, MASKL, MASKR and MERGE_BITS for each INTEGER kind,
ERFC_SCALED on both sides of its switch to the asymptotic series, and the
command-line intrinsics COMMAND_ARGUMENT_COUNT, GET_COMMAND and
EXECUTE_COMMAND_LINE. Programs run in the interpreter, at -O0 and -O2, and
with gfortran when it is installed.

Run after cargo build --bin rfort: python tests/test_command_and_bit_intrinsics.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import COMPILER


class CommandAndBitIntrinsics(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_shifts_masks_and_merge_bits(self):
        self.check('''program bits
  implicit none
  integer(1) :: b
  integer(8) :: k
  integer :: n
  print '(4i12)', shiftl(1, 3), shiftr(-16, 28), shifta(-16, 2), shifta(16, 2)
  print '(4i12)', shiftl(5, 32), shiftr(5, 32), shifta(-5, 32), shifta(5, 32)
  b = -127_1 - 1_1
  print '(4i6)', shifta(b, 3), shiftr(b, 3), shiftl(b, 1), merge_bits(12_1, 10_1, 6_1)
  k = -2_8
  print '(2i22)', shifta(k, 64), shiftr(k, 1)
  print '(4i12)', maskl(3), maskr(3), maskl(0), maskr(32)
  print '(2i22)', maskl(1, 8), maskr(63, kind=8)
  print '(3i12)', merge_bits(12, 10, 6), merge_bits(i=-1, j=0, mask=255), merge_bits(0, -1, 255)
  print '(3i12)', shiftl([1, 2, 3], 2), maskr([1, 2, 3])
  n = 5
  print '(2i12)', shifta(-64, n), shiftr(64, n)
end program bits
''', '''           8          15          -4           4
           0           0          -1           0
   -16    16     0    12
                    -1   9223372036854775807
  -536870912           7           0          -1
  -9223372036854775808   9223372036854775807
          12         255        -256
           4           8          12
           1           3           7
          -2           2
''')

    def test_shift_ranges(self):
        self.reject('print *, shiftl(1, 33)\nend\n', 'SHIFTL: SHIFT must be between 0 and 32')
        self.reject('integer :: n\nn = -1\nprint *, shifta(1, n)\nend\n', 'SHIFTA: SHIFT must be between 0 and 32')
        self.reject('print *, merge_bits(1, 2_8, 3)\nend\n', 'MERGE_BITS arguments must have the same INTEGER kind')

    def test_erfc_scaled(self):
        # Within a few units in the last place of gfortran's values.
        self.check('''program scaled
  implicit none
  real :: x4(6) = [0.5, 2.0, 8.9, 9.2, 20.0, -1.0]
  double precision :: x8(6) = [0.5d0, 2d0, 25.5d0, 26.0d0, 100d0, -1d0]
  real :: e4(6) = [6.1569029e-01, 2.5539568e-01, 6.2999293e-02, 6.0968924e-02, 2.8174350e-02, 5.0089798e+00]
  double precision :: e8(6) = [6.156903441929259d-01, 2.553956763105057d-01, 2.210810805251983d-02, &
    2.168358485056291d-02, 5.641613782989433d-03, 5.008980080762283d+00]
  print *, all(abs(erfc_scaled(x4) - e4) <= 4 * spacing(e4))
  print *, all(abs(erfc_scaled(x8) - e8) <= 4 * spacing(e8))
  print *, erfc_scaled(huge(1.0)) > 0, erfc_scaled(huge(1d0)) > 0
end program scaled
''', ' T\n T\n T T\n')

    def run_program(self, source, *arguments):
        with tempfile.TemporaryDirectory(prefix='command-line-') as tmp:
            directory = Path(tmp)
            path = directory / 'example.f90'
            path.write_text(source, encoding='utf-8')
            exe = directory / 'example.exe'
            result = subprocess.run([COMPILER, 'build', path, '-o', exe], capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stderr)
            outputs = []
            for command in ([COMPILER, 'run', path, '--', *arguments], [exe, *arguments]):
                result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=30)
                outputs.append((result.returncode, result.stdout.replace('\r\n', '\n'), result.stderr))
            return outputs

    def test_command_line(self):
        source = '''program command
  implicit none
  character(len=200) :: line
  character(len=3) :: short
  integer :: n, length, status
  n = command_argument_count()
  call get_command(line, length, status)
  print '(i0, 1x, l1, 1x, i0)', n, length == len_trim(line), status
  print '(a)', line(index(line, ' ') + 1:length)
  call get_command(short, length, status)
  print '(i0)', status
  call get_command(length=length)
  print '(l1)', length > 0
end program command
'''
        for code, stdout, stderr in self.run_program(source, 'alpha', 'b c'):
            self.assertEqual(code, 0, stderr)
            self.assertEqual(stdout, '2 T 0\nalpha b c\n-1\nT\n')

    def test_execute_command_line(self):
        source = '''program shell
  implicit none
  integer :: e, c
  character(len=60) :: msg
  print '(a)', 'before'
  call execute_command_line('echo from the shell')
  call execute_command_line('exit 3', exitstat=e, cmdstat=c)
  print '(2i4)', e, c
  e = -1
  msg = 'unset'
  call execute_command_line('exit 0', wait=.true., exitstat=e, cmdstat=c, cmdmsg=msg)
  print '(2i4, 1x, a)', e, c, trim(msg)
  call execute_command_line('exit 0', wait=.false., cmdstat=c)
  print '(i4)', c
end program shell
'''
        for code, stdout, stderr in self.run_program(source):
            self.assertEqual(code, 0, stderr)
            self.assertEqual([line.rstrip() for line in stdout.splitlines()],
                             ['before', 'from the shell', '   3   0', '   0   0 unset', '  -2'])


if __name__ == '__main__':
    unittest.main()
