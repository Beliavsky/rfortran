"""By-reference calls of external procedures: separate compilation, assumed-size
and explicit-shape dummies, sequence association, external functions,
CHARACTER arguments and DO loops with a runtime step.

Run: python tests/test_external_calls.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

# A FORTRAN 77 library, compiled separately from its callers.
LIBRARY = {
    'scalev.f': """      SUBROUTINE SCALEV(N, A, X, INCX)
      INTEGER N, INCX, I
      DOUBLE PRECISION A, X(*)
      DO 10 I = 1, 1 + (N-1)*INCX, INCX
         X(I) = A*X(I)
   10 CONTINUE
      END
""",
    'addto.f': """      SUBROUTINE ADDTO(M, N, A, LDA, S)
      INTEGER M, N, LDA, I, J
      DOUBLE PRECISION A(LDA, *), S
      DO 20 J = 1, N
         DO 10 I = 1, M
            A(I, J) = A(I, J) + S
   10    CONTINUE
   20 CONTINUE
      END
""",
    'twice.f': """      SUBROUTINE TWICE(K, V)
      INTEGER K
      DOUBLE PRECISION V
      V = 2*V + K
      END
""",
    'total.f': """      DOUBLE PRECISION FUNCTION TOTAL(N, X)
      INTEGER N, I
      DOUBLE PRECISION X(N)
      TOTAL = 0
      DO 10 I = 1, N
         TOTAL = TOTAL + X(I)
   10 CONTINUE
      END
""",
    'same.f': """      LOGICAL FUNCTION SAME(A, B)
      CHARACTER A, B
      SAME = A .EQ. B
      END
""",
    'report.f': """      SUBROUTINE REPORT(NAME, CODE)
      CHARACTER*(*) NAME
      INTEGER CODE
      WRITE(*, 100) NAME, LEN(NAME), CODE
  100 FORMAT(1X, '[', A, ']', 2I4)
      END
""",
}

PROGRAM = """program main
implicit none
double precision x(6), a(3,4), v
double precision total
logical same
external total, same
integer i, j
x = [1d0, 2d0, 3d0, 4d0, 5d0, 6d0]
call scalev(3, -1d0, x, 2)
print '(6f6.1)', x
call scalev(2, 10d0, x(5), 1)
print '(6f6.1)', x
do j = 1, 4
  do i = 1, 3
    a(i,j) = 10*i + j
  end do
end do
call addto(2, 2, a(2,3), 3, 0.5d0)
print '(4f7.1)', ((a(i,j), j = 1, 4), i = 1, 3)
v = 1
call twice(3, v)
call twice(i - 1, v)
print '(f6.1)', v
print '(f6.1)', total(4, x) + total(2, x(5))
if (same('a', 'a') .and. .not. same('a', 'b')) print *, 'characters'
i = 0
do while (total(1, x(i+1)) < 4)
  i = i + 1
end do
print *, i
call report('dgemm', 3)
call report('', -1)
end program
"""

EXPECTED = """  -1.0   2.0  -3.0   4.0  -5.0   6.0
  -1.0   2.0  -3.0   4.0 -50.0  60.0
   11.0   12.0   13.0   14.0
   21.0   22.0   23.5   24.5
   31.0   32.0   33.5   34.5
  13.0
  12.0
 characters
 3
 [dgemm]   5   3
 []   0  -1
"""


class ExternalCalls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='external-calls-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def write(self, name, text):
        path = self.directory / name
        path.write_text(text, encoding='utf-8')
        return path

    def compile(self, *args):
        return subprocess.run([COMPILER, *map(str, args)], cwd=self.directory, capture_output=True,
                              text=True, encoding='utf-8', timeout=120)

    def run_program(self, exe, code=0):
        result = subprocess.run([self.directory / exe], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return result

    def check(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_separately_compiled_library(self):
        objects = []
        for name, text in LIBRARY.items():
            source = self.write(name, text)
            obj = source.with_suffix('.obj')
            self.check(self.compile('-c', source, '-o', obj))
            objects.append(obj)
        main = self.write('main.f90', PROGRAM)
        self.check(self.compile('-c', main, '-o', 'main.obj'))
        self.check(self.compile('main.obj', *objects, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, EXPECTED)

    def test_one_compilation(self):
        # Procedures defined with their callers use the same entry points.
        source = self.write('all.f90', PROGRAM + ''.join(LIBRARY.values()))
        self.check(self.compile('-ffixed-line-length-none', source, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, EXPECTED)

    def test_runtime_do_step(self):
        source = self.write('steps.f90', """program steps
implicit none
integer i, n, step, count
n = 10
do step = -3, 3
  if (step == 0) cycle
  count = 0
  do i = n, 1, step
    count = count + 1
  end do
  do i = 1, n, step
    count = count + 100
    if (i > 7) exit
  end do
  print *, step, count, i
end do
end program
""")
        self.check(self.compile(source, '-o', 'program.exe'))
        expected = [(-3, 4, 1), (-2, 5, 1), (-1, 10, 1), (1, 800, 8), (2, 500, 9), (3, 400, 10)]
        lines = self.run_program('program.exe').stdout.split('\n')
        self.assertEqual([tuple(map(int, l.split())) for l in lines if l.strip()], expected)

    def test_fortran77_statements(self):
        source = self.write('legacy.f', """      PROGRAM LEGACY
      INTEGER K, N
      LOGICAL SELECT(3)
      CHARACTER*2 JOB
      DOUBLE PRECISION W(0:4)
      INTRINSIC REAL, MAX
      SELECT(2) = .TRUE.
      JOB(1:1) = 'S'
      JOB(2:2) = 'V'
      PRINT *, JOB, SELECT(2), REAL(MAX(1, 2))
      DO 30 K = 0, 4
         W(K) = K
         GO TO (10, 20) K
         PRINT *, 'none', K
         GO TO 30
   10    PRINT *, 'one'
         GO TO 30
   20    PRINT *, 'two'
   30 CONTINUE
      N = 3
      CALL SHIFT(N, W(1))
      PRINT *, W
      END
      SUBROUTINE SHIFT(N, V)
      INTEGER N, I
      DOUBLE PRECISION V(0:*)
      DO 10 I = N, 1, -1
         V(I) = V(I-1)
   10 CONTINUE
      END
""")
        self.check(self.compile(source, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout.split('\n'), [
            ' SV T 2.0', ' none 0', ' one', ' two', ' none 3', ' none 4', ' 0.0 1.0 1.0 2.0 3.0', ''])

    def test_procedure_features(self):
        # Each procedure is compiled on its own, as a library.
        library = self.write('features.f', """      SUBROUTINE TABLE(K, V)
      INTEGER K, V, IPIV(2, 3), I
      DOUBLE PRECISION CR(2, 2), CRV(4)
      EQUIVALENCE (CR(1, 1), CRV(1))
      DATA IPIV / 1, 2, 3, 4, 5, 6 /
      DO 10 I = 1, 4
         CRV(I) = I
   10 CONTINUE
      V = IPIV(2, K) + INT(CR(2, 2))
      END
      CHARACTER FUNCTION GRADE(X)
      DOUBLE PRECISION X
      GRADE = 'B'
      IF (X .GT. 0.5D0) GRADE = 'A'
      END
      SUBROUTINE PEAK(N, W, LOC, LAST)
      INTEGER N, LOC, LAST
      DOUBLE PRECISION W(*), SUM(2)
      SUM(1) = 0
      DO 10 LAST = 1, N
         SUM(1) = SUM(1) + W(LAST)
   10 CONTINUE
      LOC = MAXLOC(W(1:N), 1)
      LAST = LAST + INT(SUM(1))
      END
      SUBROUTINE AUTO(M, N, X)
      INTEGER M, N, I
      DOUBLE PRECISION X(*), WORK(MAX(M, N))
      DO 10 I = 1, MAX(M, N)
         WORK(I) = 10*I
   10 CONTINUE
      X(1) = WORK(MAX(M, N))
      END
""")
        self.check(self.compile('-c', library, '-o', 'features.obj'))
        main = self.write('main.f90', """program main
implicit none
integer v, loc, last
double precision w(5)
character grade
external grade
call table(3, v)
print *, v
print *, grade(0.7d0), grade(0.1d0)
w = [1d0, 4d0, 2d0, 4d0, 3d0]
call peak(5, w, loc, last)
print *, loc, last
call auto(2, 3, w)
print *, w(1)
end program
""")
        self.check(self.compile('-c', main, '-o', 'main.obj'))
        self.check(self.compile('main.obj', 'features.obj', '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, ' 10\n AB\n 2 20\n 30.0\n')

    def test_bounds(self):
        source = self.write('bounds.f90', """program bounds
implicit none
double precision x(4)
x = 1
call scalev(3, 2d0, x, 2)
end program
""" + LIBRARY['scalev.f'])
        self.check(self.compile('-ffixed-line-length-none', source, '-o', 'program.exe'))
        result = self.run_program('program.exe', code=1)
        self.assertIn('subscript outside bounds', result.stdout + result.stderr)
        source = self.write('larger.f90', """program larger
implicit none
double precision x(3)
x = 1
print *, total(4, x)
end program
""" + LIBRARY['total.f'])
        self.check(self.compile('-ffixed-line-length-none', source, '-o', 'program.exe'))
        result = self.run_program('program.exe', code=1)
        self.assertIn('larger than its actual argument', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
