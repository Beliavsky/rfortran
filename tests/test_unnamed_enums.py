"""Unnamed ENUM, BIND(C) (Fortran 2003): enumerators are named INTEGER
constants of the kind of a C int, numbered from zero or from a given value,
with or without `::` and several to a statement, in modules and USEd
elsewhere, and usable as kinds, bounds and CASE values.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_unnamed_enums.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class UnnamedEnums(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_values(self):
        self.check("""program p
  use iso_c_binding
  implicit none
  enum, bind(c)
    enumerator :: red = 1, blue
    enumerator green
    enumerator :: big = 2**20, small = -3, next
  end enum
  enum, bind(c)
    enumerator :: zero, one, two
  end enum
  integer(kind(red)) :: c
  c = blue
  print *, red, blue, green, big, small, next, c, kind(red) == c_int
  print *, zero, one, two
end program
""", "1 2 3 1048576 -3 -2 2 T\n0 1 2\n")

    def test_module_constants(self):
        self.check("""module colors
  implicit none
  enum, bind(c)
    enumerator :: none = 0, warm = 10, cool, neutral
  end enum
end module
program p
  use colors
  implicit none
  integer :: table(none:neutral), k
  table = 0
  table(cool) = 7
  do k = warm, neutral
    select case (k)
    case (warm)
      print *, 'warm', table(k)
    case (cool)
      print *, 'cool', table(k)
    case default
      print *, 'other', k
    end select
  end do
end program
""", "warm 0\ncool 7\nother 12\n")

    def test_rejection(self):
        self.reject("""program p
  enum, bind(fortran)
    enumerator :: a
  end enum
end program
""", "an ENUM statement requires BIND(C)")


if __name__ == "__main__":
    unittest.main()
