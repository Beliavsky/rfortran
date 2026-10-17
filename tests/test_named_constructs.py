"""Named IF, SELECT CASE, BLOCK and ASSOCIATE constructs, and EXIT from them.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_named_constructs.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class NamedConstructs(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_names_and_exits(self):
        self.check("""program p
implicit none
integer :: i, j, k, n
n = 0
check: if (n == 0) then
  print *, 'zero'
else if (n > 0) then check
  print *, 'pos'
else check
  print *, 'neg'
end if check
pick: select case (n)
case (0) pick
  print *, 'case 0'
case default pick
  print *, 'other'
end select pick
search: block
  do i = 1, 10
    if (i == 4) exit search
    print *, 'i', i
  end do
  print *, 'not reached'
end block search
print *, 'after search', i
outer: do i = 1, 3
  inner: block
    if (i == 2) cycle
    if (i == 3) exit
    if (i == 1) exit inner
    print *, 'never'
  end block inner
  print *, 'end of iteration', i
end do outer
print *, 'after outer', i
do j = 1, 2
  test: if (j > 0) then
    k = j
    if (j == 1) exit test
    print *, 'j', j
  endif test
end do
named: associate (z => n + 1)
  print *, 'z', z
end associate named
print *, 'k', k
end program
""", "zero\ncase 0\ni 1\ni 2\ni 3\nafter search 4\nend of iteration 1\n"
     "after outer 3\nj 2\nz 1\nk 2\n")

    def test_nested_exits_reach_enclosing_loops(self):
        self.check("""program p
implicit none
integer :: i, j, total
total = 0
rows: do i = 1, 4
  first: block
    second: if (i > 0) then
      do j = 1, 3
        if (j == 2) cycle
        total = total + 10*i + j
      end do
      if (i == 2) exit first
      if (i == 3) cycle rows
      if (i == 4) exit
      total = total + 1000
    end if second
    total = total + 100000
  end block first
end do rows
print *, total, i
end program
""", "101216 4\n")

    def test_block_locals_shadow_outer_variables(self):
        self.check("""program p
implicit none
integer :: k = 1, a(2) = [1, 1]
character(len=3) :: c = 'out'
call s()
block
  integer :: k, a(2)
  character(len=3) :: c
  k = 9
  a = 9
  c = 'in'
  block
    integer :: k
    k = 5
  end block
  print *, k, a, c
end block
print *, k, a, c
contains
subroutine s()
  integer, save :: i = -100
  scan: block
    integer :: I
    do i = 1, 3
      if (i == 2) exit scan
    end do
  end block scan
  print *, i
end subroutine s
end program
""", "-100\n9 9 9 in\n1 1 1 out\n")


if __name__ == "__main__":
    unittest.main()
