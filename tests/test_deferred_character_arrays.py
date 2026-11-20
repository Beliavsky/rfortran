"""Deferred-length CHARACTER arrays whose length is set at run time: ALLOCATE
with a runtime length, reallocation on assignment of values of other lengths
and sizes, growth by constructors and procedure and BLOCK locals.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_deferred_character_arrays.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import COMPILER


class DeferredCharacterArrays(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_runtime_length_allocation(self):
        self.check("""program p
implicit none
character(len=:), allocatable :: w(:)
integer :: n, i
n = 5
allocate(character(len=n) :: w(3))
w(1) = 'abc'
w(2) = 'toolongvalue'
w(3) = ''
print '(3("[",a,"]"))', w
print *, len(w), size(w), allocated(w)
deallocate(w)
print *, allocated(w)
do i = 1, 3
  allocate(character(len=i*2) :: w(i))
  w = 'xyzxyzxyz'
  print '(3(a,"|"))', w
  print *, len(w), size(w), len_trim(w(1))
  deallocate(w)
end do
end program
""", "[abc  ][toolo][     ]\n 5 3 T\n F\nxy|\n 2 1 2\nxyzx|xyzx|\n 4 2 4\nxyzxyz|xyzxyz|xyzxyz|\n 6 3 6\n")

    def test_assignment_reallocates_to_length_and_size(self):
        self.check("""program p
implicit none
character(len=:), allocatable :: w(:), v(:)
character(len=3) :: fixed(4) = ['one', 'two', 'six', 'ten']
w = [character(len=4) :: 'ab', 'cd']
print *, len(w), size(w)
w = [character(len=6) :: 'x', 'y', 'z']
print *, len(w), size(w)
w = ['red  ', 'green', 'blue ']
print '(3("[",a,"]"))', w
print *, trim(w(2)) // '!', w(2)(2:3), index(w(2), 'e')
w = fixed
print *, len(w), size(w), w(4)
w = [character(len=7) :: 'alpha', 'beta']
v = w
print *, len(v), size(v), '[', v(1), ']'
w = repeat('z', 10)
print *, w
end program
""", " 4 2\n 6 3\n[red  ][green][blue ]\n green!re 3\n 3 4 ten\n 7 2 [alpha  ]\n zzzzzzzzzzzzzz\n", interpreter=False)

    def test_values_that_refer_to_the_array(self):
        self.check("""program p
implicit none
character(len=:), allocatable :: w(:), v(:)
integer :: i
w = [character(len=5) :: 'alpha', 'beta']
w = [w(2), w(1)]
print '(2("[",a,"]"))', w
allocate(character(len=3) :: v(0))
do i = 1, 3
  v = [v, repeat(achar(iachar('a') + i - 1), 3)]
end do
print *, size(v), len(v), v
v = [v(3), w(1)(1:3)]
print '(2("[",a,"]"))', v
end program
""", "[beta ][alpha]\n 3 3 aaabbbccc\n[ccc][bet]\n", interpreter=False)

    def test_constructors_with_section_items(self):
        # Sections of the array itself, with strides, omitted and run-time
        # bounds, and empty sections, and sections of other arrays. The
        # interpreter copies each element's whole storage, which is slow.
        self.check("""program p
implicit none
character(len=:), allocatable :: names(:), other(:)
character(len=3) :: fixed(4)
integer :: k
fixed = ['aaa', 'bbb', 'ccc', 'ddd']
names = ['ab', 'cd', 'gh', 'ij']
names = [names(::2), names(4:1:-2)]
print *, size(names), len(names), '[', names, ']'
k = 3
names = [names(k:), 'zz', names(:k-2)]
print *, size(names), len(names), '[', names, ']'
names = [character(len=4) :: names(2:3), 'wxyz']
print *, size(names), len(names), '[', names, ']'
other = [names(1:0), names(3:3)]
print *, size(other), len(other), '[', other, ']'
other = [fixed(2:4:2), fixed(1:1)]
print *, size(other), len(other), '[', other, ']'
names = [other(2:), other(1:1)]
print *, size(names), len(names), '[', names, ']'
names = [names(2:1)]
print *, size(names), len(names)
end program
""", "4 2 [abghijcd]\n4 2 [ijcdzzab]\n3 4 [cd  zz  wxyz]\n1 4 [wxyz]\n3 3 [bbbdddaaa]\n3 3 [dddaaabbb]\n0 3\n", interpreter=False)

    def test_function_results(self):
        # Module, renamed and internal functions; a RETURN before the end;
        # references in assignments, output lists, IF conditions, DO bounds
        # and arguments; and one result passed to another function. The
        # interpreter copies each element's whole storage, which is slow.
        self.check("""module words_mod
implicit none
contains
function split(s) result(words)
character(len=*), intent(in) :: s
character(len=:), allocatable :: words(:)
integer :: i, j, n
allocate (character(len=0) :: words(0))
i = 1
do while (i <= len(s))
  if (s(i:i) == ' ') then
    i = i + 1
    cycle
  end if
  j = index(s(i:) // ' ', ' ') - 1
  n = max(len(words), j)
  words = [character(len=n) :: words, s(i:i+j-1)]
  i = i + j
end do
end function
function repeat_all(a, k)
character(len=*), intent(in) :: a(:)
integer, intent(in) :: k
character(len=:), allocatable :: repeat_all(:)
integer :: i
allocate (character(len=k*len(a)) :: repeat_all(size(a)))
if (k == 0) return
do i = 1, size(a)
  repeat_all(i) = repeat(a(i), k)
end do
end function
end module
program p
use words_mod, only: sp => split, repeat_all
implicit none
character(len=:), allocatable :: w(:)
integer :: i
w = sp('ab cdef g')
print *, size(w), len(w), '[', w, ']'
print *, size(sp('x yy zzz')), len(sp('x yy zzz'))
print *, '[', repeat_all(sp('ab c'), 2), ']'
w = repeat_all(w, 0)
print *, size(w), len(w)
if (size(sp('a b c d')) == 4) print *, 'four'
print *, '[', twice(['ab', 'cd']), ']'
do i = 1, size(sp('p q'))
  print *, i
end do
contains
function twice(a)
character(len=*), intent(in) :: a(:)
character(len=:), allocatable :: twice(:)
twice = [a, a]
end function
end program
""", "3 4 [ab  cdefg   ]\n3 3\n[ababc c ]\n3 0\nfour\n[abcdabcd]\n1\n2\n", interpreter=False)

    # Results of one length throughout, results in typed constructors, in
    # a DO loop, a BLOCK and a fixed-length target, and a module function
    # using another module's.
    RESULT_MODULES = """module a_mod
implicit none
contains
function pair() result(r)
character(len=:), allocatable :: r(:)
r = ['ab', 'cd']
end function
function upto(n)
integer, intent(in) :: n
character(len=:), allocatable :: upto(:)
integer :: i
allocate (character(len=n) :: upto(n))
do i = 1, n
  upto(i) = repeat('x', i)
end do
end function
end module
module b_mod
use a_mod
implicit none
contains
function both(n) result(r)
integer, intent(in) :: n
character(len=:), allocatable :: r(:)
r = [character(len=n) :: pair(), upto(n)]
end function
end module
"""
    RESULT_PROGRAM = """program p
use b_mod
implicit none
character(len=:), allocatable :: w(:)
character(len=5), allocatable :: v(:)
integer :: i
do i = 1, 3
  w = upto(i)
  print *, size(w), len(w), '[', w, ']'
end do
block
  character(len=:), allocatable :: q(:)
  q = pair()
  print *, size(q), len(q), '[', q, ']'
end block
v = upto(2)
print *, size(v), len(v), '[', v, ']'
w = [character(len=3) :: upto(1), pair()]
print *, size(w), len(w), '[', w, ']'
print *, '[', both(3), ']'
print *, len(both(4)), size(both(4))
end program
"""
    RESULT_OUTPUT = "1 1 [x]\n2 2 [x xx]\n3 3 [x  xx xxx]\n2 2 [abcd]\n2 5 [x    xx   ]\n3 3 [x  ab cd ]\n[ab cd x  xx xxx]\n4 6\n"

    def test_function_results_of_used_modules(self):
        self.check(self.RESULT_MODULES + self.RESULT_PROGRAM, self.RESULT_OUTPUT, interpreter=False)

    def test_function_results_of_modules_compiled_separately(self):
        with tempfile.TemporaryDirectory(prefix="deferred-") as tmp:
            d = Path(tmp)
            modules = self.RESULT_MODULES.split("module b_mod")
            (d / "a.f90").write_text(modules[0])
            (d / "b.f90").write_text("module b_mod" + modules[1])
            (d / "main.f90").write_text(self.RESULT_PROGRAM)
            run = lambda *args: subprocess.run([str(COMPILER), *map(str, args)], cwd=d, capture_output=True, text=True, timeout=120)
            for step in [("-c", "a.f90", "-o", "a.obj"), ("-c", "b.f90", "-o", "b.obj"), ("main.f90", "a.obj", "b.obj", "-o", "main.exe")]:
                result = run(*step)
                self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(d / "main.exe")], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.stdout.split(), self.RESULT_OUTPUT.split())

    def test_block_locals_with_runtime_length(self):
        self.check("""program p
implicit none
character(len=:), allocatable :: w(:)
integer :: n
n = 3
allocate (character(len=n) :: w(2))
w(1) = 'abc'; w(2) = 'de'
block
  character(len=:), allocatable :: q(:)
  q = w
  q = [character(len=n+1) :: q, 'z']
  print *, size(q), len(q), '[', q, ']'
end block
end program
""", "3 4 [abc de  z   ]\n", interpreter=False)

    def test_procedure_and_block_locals(self):
        self.check("""program p
call s(4)
call s(2)
block
  character(len=:), allocatable :: b(:)
  allocate(character(len=4) :: b(2))
  write(b(1), '(i4)') 42
  b(2) = 'ok'
  print '(2("[",a,"]"))', b
end block
contains
subroutine s(n)
integer, intent(in) :: n
character(len=:), allocatable :: w(:)
integer :: i
allocate(character(len=n) :: w(n))
do i = 1, n
  write(w(i), '(i0)') i*i
end do
print '(*(a,:,","))', w
end subroutine
end program
""", "1   ,4   ,9   ,16  \n1 ,4 \n[  42][ok  ]\n")

    def test_length_beyond_storage_capacity(self):
        self.check("""program p
character(len=:), allocatable :: w(:)
integer :: n
n = 5000
allocate(character(len=n) :: w(1))
print *, len(w)
end program
""", error="storage capacity")


if __name__ == "__main__":
    unittest.main(verbosity=2)
