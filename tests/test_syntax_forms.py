"""Common statement forms: calls and subroutines without argument lists,
program units after the main program, bare END, IMPLICIT NONE (TYPE,
EXTERNAL), DEALLOCATE lists, continued CHARACTER literals, DIMENSION
statements, TYPE(t) FUNCTION and empty source files.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_syntax_forms.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import build_commands


class SyntaxForms(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_calls_and_subroutines_without_argument_lists(self):
        self.check("""module m
implicit none
integer :: count = 0
contains
subroutine bump
  count = count + 1
end subroutine bump
end
program main
use m
implicit none
call bump
call bump()
if (count > 1) call bump
call report
contains
subroutine report
  print *, count
end subroutine
end program
""", "3\n")

    def test_units_after_the_main_program(self):
        source = """program main
implicit none
integer, external :: twice
call hello
print *, twice(4)
end program main
integer function twice(i)
  integer, intent(in) :: i
  twice = 2*i
end function twice
subroutine hello
  print *, 'hello'
end subroutine hello
"""
        self.check(source, "hello\n8\n")

    def test_implicit_none_specs_and_deallocate_lists(self):
        self.check("""program main
implicit none (type, external)
real, allocatable :: x(:), y(:)
allocate(x(2), y(3))
x = 1
y = 2
print *, sum(x) + sum(y)
deallocate(x, y)
print *, allocated(x), allocated(y)
call s
contains
subroutine s
  implicit none (external)
  print *, 'ok'
end subroutine s
end program
""", "8.0\nF F\nok\n")

    def test_continued_character_literals(self):
        self.check("""program main
implicit none
print *, "Hello, &
   &world"
print *, 'one &
  &two', len("a&b")
print *, 'x' // &
  'y'
end program
""", "Hello, world\none two 3\nxy\n")

    def test_dimension_statements(self):
        self.check("""program main
implicit none
real :: y
integer x
character(len=2) :: c
dimension y(2), x(0:2)
dimension c(2)
y = [1.5, 2.5]
x = [7, 8, 9]
c = ['ab', 'cd']
print *, y, x, lbound(x), c
call s(3)
contains
subroutine s(n)
  integer, intent(in) :: n
  integer :: w
  dimension w(n)
  w = n
  print *, w
end subroutine s
end program
""", "1.5 2.5 7 8 9 0 abcd\n3 3 3\n")

    def test_derived_type_function_prefix(self):
        self.check("""module m
implicit none
type :: v
  real :: x
end type v
contains
type(v) function mk(x)
  real, intent(in) :: x
  mk%x = x
end function mk
type(v) function twice(a) result(r)
  type(v), intent(in) :: a
  r%x = 2*a%x
end function twice
end module m
program main
use m
implicit none
type(v) :: a
a = twice(mk(2.0))
print *, a%x
end program
""", "4.0\n")

    def test_keywords_written_without_blanks(self):
        self.check("""module m
implicit none
type :: pt
  integer :: x
endtype pt
contains
pure doubleprecision function half(x)
  doubleprecision, intent(in) :: x
  half = x/2
endfunction half
subroutine show(i)
  integer, intent(in) :: i
  selectcase (i)
  case (1)
    print *, 'one'
  case default
    print *, 'other'
  endselect
endsubroutine show
endmodule m
program p
use m
implicit none
doubleprecision :: d
integer :: i, endif
d = 3.0d0
print *, half(d)
do i = 1, 3
  if (i == 1) then
    call show(i)
  elseif (i == 2) then
    call show(i)
  else
    print *, 'else'
  endif
enddo
endif = 5
print *, endif
block
  print *, 'blk'
endblock
endprogram p
""", "1.5\none\nother\nelse\n5\nblk\n")

    def test_names_are_case_insensitive(self):
        self.check("""PROGRAM Main
IMPLICIT NONE
INTEGER, PARAMETER :: DP = KIND(1.0D0)
INTEGER :: Count
REAL(dp) :: X(2)
count = 3
x = 1.5_DP
CALL Show(COUNT, X)
CONTAINS
SUBROUTINE SHOW(n, y)
  integer, intent(IN) :: N
  real(DP), intent(in) :: Y(:)
  print *, n, SUM(y), Kind(Y)
END SUBROUTINE show
END PROGRAM main
""", "3 3.0 8\n")

    def test_boz_literals_and_unsigned_comparisons(self):
        self.check("""program p
use iso_fortran_env, only: int8, int16
implicit none
integer :: i
integer(int16) :: s
i = int(b'101') + int(o'17') + int(z'ff')
s = int(b'1010101010101010', kind=int16)
print *, i, s, int(b'00100100', int8), int(z'FFFFFFFF')
print *, bge(b'1', 2), bgt(-1, 1), ble(5, 5), blt(1, -1)
end program
""", "275 -21846 36 -1\nF T T T\n")

    def test_operators_after_integer_literals(self):
        self.check("""program p
implicit none
integer :: i = 97
print *, i>=97.and.i<=122, 1.eq.1, 2.ne.2, 1.e1, 3.
end program
""", "T T F 10.0 3.0\n")

    def test_empty_and_comment_only_files(self):
        with tempfile.TemporaryDirectory(prefix="syntax-forms-") as tmp:
            tmp = Path(tmp)
            empty = tmp / "empty.f90"
            empty.write_text("", encoding="utf-8")
            comments = tmp / "comments.f90"
            comments.write_text("! only a comment\n\n", encoding="utf-8")
            main = tmp / "main.f90"
            main.write_text("program main\nprint *, 1\nend program\n", encoding="utf-8")
            compiler = build_commands(main, tmp)[0][0]
            for source in (empty, comments):
                with self.subTest(source=source.name):
                    result = subprocess.run([compiler, "check", source], capture_output=True, text=True, timeout=60)
                    self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([compiler, "-O0", "--quiet", empty, main, "-o", tmp / "main.exe", "--run"],
                                    capture_output=True, text=True, timeout=120, cwd=tmp)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.split(), ["1"])


if __name__ == "__main__":
    unittest.main()
