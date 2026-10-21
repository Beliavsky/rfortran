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

    def test_complex(self):
        # COMPLEX scalars, arrays, elements, constants and function results,
        # with the FORTRAN 77 specific names DCONJG, DIMAG and DCMPLX.
        library = self.write('zlib.f', """      SUBROUTINE ZAXPYS(N, ZA, ZX, ZY)
      INTEGER N, I
      COMPLEX*16 ZA, ZX(*), ZY(*)
      DO 10 I = 1, N
         ZY(I) = ZY(I) + ZA*ZX(I)
   10 CONTINUE
      END
      COMPLEX*16 FUNCTION ZDOTCS(N, ZX, ZY)
      INTEGER N, I
      COMPLEX*16 ZX(*), ZY(*)
      ZDOTCS = (0.0D0, 0.0D0)
      IF (N .LE. 0) RETURN
      DO 10 I = 1, N
         ZDOTCS = ZDOTCS + DCONJG(ZX(I))*ZY(I)
   10 CONTINUE
      END
      DOUBLE PRECISION FUNCTION CABS1S(Z)
      COMPLEX*16 Z
      CABS1S = ABS(DBLE(Z)) + ABS(DIMAG(Z))
      END
      SUBROUTINE ZROT90(Z)
      DOUBLE COMPLEX Z
      Z = DCMPLX(-DIMAG(Z), DBLE(Z))
      END
""")
        self.check(self.compile('-c', library, '-o', 'zlib.obj'))
        main = self.write('main.f90', """program main
implicit none
complex*16 x(4), y(4), zdotcs
double precision cabs1s
external zdotcs, cabs1s, zaxpys, zrot90
integer i
do i = 1, 4
  x(i) = dcmplx(dble(i), 1d0)
  y(i) = dcmplx(0d0, dble(i))
end do
call zaxpys(3, (0d0, 2d0), x, y(2))
call zrot90(x(4))
print '(8f7.2)', y
print '(8f7.2)', x
print '(2f8.2)', zdotcs(4, x, y), zdotcs(0, x, y)
print '(f8.2)', cabs1s(y(3)) + cabs1s((3d0, -4d0))
end program
""")
        self.check(self.compile('-c', main, '-o', 'main.obj'))
        self.check(self.compile('main.obj', 'zlib.obj', '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, """   0.00   1.00  -2.00   4.00  -2.00   7.00  -2.00  10.00
   1.00   1.00   2.00   1.00   3.00   1.00  -1.00   4.00
   44.00   32.00
    0.00    0.00
   16.00
""")

    def test_statement_functions_and_goto_dispatch(self):
        # A computed GOTO into the middle of a backward loop needs a state
        # machine; statement functions may have COMPLEX dummies.
        source = self.write('dispatch.f', """      PROGRAM SF
      DOUBLE PRECISION F, T, CABS1
      COMPLEX*16 ZDUM, Z
      INTEGER K, ISAVE, N
      F(T) = 2*T + 1
      CABS1(ZDUM) = ABS(DBLE(ZDUM)) + ABS(DIMAG(ZDUM))
      Z = (3.0D0, -4.0D0)
      PRINT '(3F8.2)', F(1.5D0), CABS1(Z), F(CABS1(Z))
      DO 5 ISAVE = 1, 3
         N = 0
         K = 0
         GO TO (10, 20, 30) ISAVE
   10    CONTINUE
         N = N + 1
   20    CONTINUE
         N = N + 10
         K = K + 1
   30    CONTINUE
         N = N + 100
         IF (K .LT. 2) GO TO 20
         PRINT '(2I5)', N, K
    5 CONTINUE
      END
""")
        self.check(self.compile(source, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout,
                         '    4.00    7.00   15.00\n  221    2\n  220    2\n  320    2\n')

    def test_save_data_and_dummy_sections(self):
        # SAVE with and without a list, a variable named IF, DATA with
        # implied-DO lists, and elemental operations on sections of dummies.
        source = self.write('save.f', """      PROGRAM SD
      INTEGER I, J, NEXT, ISTATE
      COMPLEX*16 A( 3, 3 ), QC( 2, 3 )
      DOUBLE PRECISION R( 3, 3 ), X( 3 )
      ISTATE = 0
      DO 10 I = 1, 3
         J = NEXT( ISTATE )
         PRINT '(2I5)', J, ISTATE
   10 CONTINUE
      CALL TABLE
      DO 30 J = 1, 3
         DO 20 I = 1, 3
            A( I, J ) = DCMPLX( I, J )
            R( I, J ) = I*J
   20    CONTINUE
         QC( 1, J ) = DCMPLX( J, -2*J )
         QC( 2, J ) = 0
   30 CONTINUE
      X( 1 ) = -1.25D0
      X( 2 ) = 0.25D0
      X( 3 ) = 2.5D0
      CALL SECT( 3, A, 3, QC, 2, R, X )
      PRINT '(6F8.3)', DBLE( A( 1, 2 ) ), DIMAG( A( 1, 2 ) ),
     $   DBLE( A( 2, 2 ) ), DIMAG( A( 2, 2 ) ), DBLE( A( 3, 2 ) ),
     $   DIMAG( A( 3, 2 ) )
      PRINT '(3F9.4)', R
      END
*     A counter whose locals keep their values between calls.
      INTEGER FUNCTION NEXT( ISTATE )
      INTEGER ISTATE, K, IF
      DOUBLE PRECISION W
      SAVE
      IF( ISTATE.EQ.0 ) THEN
         K = 10
         W = 0.5D0
      END IF
      IF = K
      K = K + 1
      W = 2*W
      ISTATE = INT( W )
      NEXT = IF
      END
      SUBROUTINE TABLE
      INTEGER LV, J, K, CALLS
      PARAMETER ( LV = 3 )
      INTEGER MM( LV, 2 ), V( 4 )
      SAVE CALLS
      DATA ( MM( 1, J ), J = 1, 2 ) / 11, 12 /,
     $     ( MM( 2, J ), J = 1, 2 ) / 2*21 /
      DATA ( ( MM( K, J ), J = 1, 2 ), K = 3, 3 ) / 31, 32 /
      DATA V / 4*7 /, CALLS / 0 /
      CALLS = CALLS + 1
      PRINT '(8I4)', ( ( MM( K, J ), K = 1, LV ), J = 1, 2 ), V( 3 ),
     $   CALLS
      END
*     Elemental operations on sections of dummy arrays.
      SUBROUTINE SECT( N, A, LDA, QC, LDQC, R, X )
      INTEGER N, LDA, LDQC
      COMPLEX*16 A( LDA, * ), QC( LDQC, * )
      DOUBLE PRECISION R( LDA, * ), X( * )
      DOUBLE PRECISION, ALLOCATABLE :: W( : )
      ALLOCATE( W( N ) )
      W = X( 1:N )
      A( 1:N, 2 ) = A( 1, 2 )*DCONJG( QC( 1, 1:N ) )
      R( 1, 1:N ) = 2*W
      R( 2, 1:N ) = ABS( X( 1:N ) ) + MAX( X( 1:N ), 0.5D0 )
      R( 3, 1:N ) = DBLE( INT( X( 1:N )*10 ) ) + SQRT( ABS( X( 1:N ) ) )
      END
""")
        self.check(self.compile(source, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, '   10    1\n   11    2\n   12    4\n  11  21  31  12  21  32   7   1\n  -3.000   4.000  -6.000   8.000  -9.000  12.000\n  -2.5000   1.7500 -10.8820\n   0.5000   0.7500   2.5000\n   5.0000   5.0000  26.5811\n')

    def test_functions_defining_arguments(self):
        # Functions that change scalar arguments, which the caller sees:
        # module, internal and external ones, in expressions, conditions
        # and DO WHILE conditions.
        source = self.write('defining.f90', """module counters
  implicit none
  private
  public :: bump, take
contains
  integer function bump(k)
    integer, intent(inout) :: k
    k = k + 1
    bump = 10 * k
  end function
  function take(n, step) result(r)
    integer :: n, step, r
    r = n
    if (n <= 0) return
    n = n - step
    r = -n
  end function
end module

program f7
  use counters, only: bump, take
  implicit none
  integer :: i, j, m, total
  i = 0
  j = bump(i) + bump(i)
  print '(2i6)', j, i
  if (bump(i) > 25) print '(a,i4)', 'big ', i
  m = 7
  total = 0
  do while (take(m, 3) < 0)
    total = total + 1
  end do
  print '(3i6)', m, total, take(m, 1)
  print '(2i6)', bump(j), j
  call inner(i)
  print '(i6)', i
contains
  subroutine inner(k)
    integer :: k
    k = twice(k) + twice(k)
  end subroutine
  integer function twice(k)
    integer :: k
    k = 2 * k
    twice = k
  end function
end program
""")
        self.check(self.compile(source, '-o', 'program.exe'))
        self.assertEqual(self.run_program('program.exe').stdout, '    30     2\nbig    3\n    -2     2    -2\n   310    31\n    18\n')
        main = self.write('main.f', """      PROGRAM FA
      INTEGER I, J, NX
      I = 0
      J = NX( I )
      PRINT '(2I3)', J, I
      END
""")
        function = self.write('nx.f', """      INTEGER FUNCTION NX( K )
      INTEGER K
      K = K + 5
      NX = 1
      END
""")
        self.check(self.compile(main, function, '-o', 'one.exe'))
        self.assertEqual(self.run_program('one.exe').stdout, '  1  5\n')
        self.check(self.compile('-c', function, '-o', 'nx.obj'))
        self.check(self.compile(main, 'nx.obj', '-o', 'two.exe'))
        self.assertEqual(self.run_program('two.exe').stdout, '  1  5\n')

    def test_transfer_and_automatic_arrays(self):
        # TRANSFER of a REAL workspace to a COMPLEX dummy, as in LAPACK's
        # xxxRFSX routines, in a loop; and automatic arrays, REAL and
        # COMPLEX, of procedures with by-reference entry points.
        main = self.write('main.f90', """program tr
  implicit none
  double precision :: r(6)
  integer :: i
  r = [(dble(i), i = 1, 6)]
  call s(3, r)
  call a(3, r)
  print '(6f5.1)', r
end program
""")
        library = self.write('library.f', """      SUBROUTINE S( N, RWORK )
      INTEGER N, K
      DOUBLE PRECISION RWORK( * )
      DOUBLE PRECISION ZERO
      PARAMETER ( ZERO = 0.0D+0 )
      EXTERNAL G
      INTRINSIC TRANSFER
      DO K = 1, 2
         CALL G( N-K+1, TRANSFER( RWORK( K:K+2*N-1 ),
     $           (/ (ZERO, ZERO) /), N-K+1 ) )
      END DO
      END
      SUBROUTINE A( N, RWORK )
      INTEGER N, I
      DOUBLE PRECISION RWORK( * )
      DOUBLE PRECISION X( N ), Y( N )
      COMPLEX*16 T( N )
      EXTERNAL G
      DO I = 1, N
         X( I ) = RWORK( 2*I-1 )
         Y( I ) = RWORK( 2*I )
         T( I ) = DCMPLX( Y( I ), X( I ) )
      END DO
      DO I = 1, N
         WRITE( *, '(A,2F5.1)' ) 'A', X( I ), Y( I )
      END DO
      CALL G( N, T )
      END
      SUBROUTINE G( N, Z )
      INTEGER N, I
      COMPLEX*16 Z( * )
      DO I = 1, N
         WRITE( *, '(A,2F5.1)' ) 'G', DBLE( Z( I ) ), DIMAG( Z( I ) )
         Z( I ) = 0
      END DO
      END
""")
        # From gfortran.
        expected = ('G  1.0  2.0\nG  3.0  4.0\nG  5.0  6.0\nG  2.0  3.0\nG  4.0  5.0\n'
                    'A  1.0  2.0\nA  3.0  4.0\nA  5.0  6.0\nG  2.0  1.0\nG  4.0  3.0\nG  6.0  5.0\n'
                    '  1.0  2.0  3.0  4.0  5.0  6.0\n')
        self.check(self.compile(main, library, '-o', 'one.exe'))
        self.assertEqual(self.run_program('one.exe').stdout, expected)
        self.check(self.compile('-c', library, '-o', 'library.obj'))
        self.check(self.compile(main, 'library.obj', '-o', 'two.exe'))
        self.assertEqual(self.run_program('two.exe').stdout, expected)

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
