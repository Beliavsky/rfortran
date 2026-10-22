"""Deferred-length CHARACTER arrays whose length is set at run time: ALLOCATE
with a runtime length, reallocation on assignment of values of other lengths
and sizes, growth by constructors and procedure and BLOCK locals.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_deferred_character_arrays.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


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
