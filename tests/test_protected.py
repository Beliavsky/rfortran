"""The PROTECTED attribute and statement: module variables that only their
module may define. Units that use them may read them; assigning them, their
elements or components, reading into them (also by IOSTAT=), using them as DO
variables, passing them to INTENT(OUT) or INTENT(INOUT) dummies, allocating
them and changing the association of PROTECTED pointers are rejected, while
the target of a PROTECTED pointer may be defined.

Each valid case is checked in the interpreter, at -O0 and -O2, and against
gfortran.
Run after cargo build --bin rfort: python tests/test_protected.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate

MODULE = """module counters
  implicit none
  integer, protected :: count = 0
  real, protected :: table(3)
  integer, pointer, protected :: current => null()
  integer, allocatable, protected :: sizes(:)
  type pair
    integer :: a, b
  end type
  type(pair) :: last
  protected :: last
contains
  subroutine bump()
    count = count + 1
    table(count) = 1.5 * count
    last = pair(count, 2 * count)
  end subroutine
  subroutine set(x)
    integer, intent(out) :: x
    x = 1
  end subroutine
end module
"""


class Protected(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_module_defines_and_users_read(self):
        self.check(MODULE + """module reports
  use counters, only: total => count, last
contains
  integer function doubled()
    doubled = 2 * total
  end function
end module
program p
  use counters
  use reports
  implicit none
  integer :: k
  call bump()
  call bump()
  k = count + 1
  print *, count, k, table(1:2), last%a, last%b, doubled()
  if (associated(current)) current = 5
end program
""", "2 3 1.5 3.0 2 4 4\n")

    def test_rejections(self):
        program = "program p\n  use counters\n  implicit none\n  integer :: k\n  {}\nend program\n"
        cases = [
            ("count = 5", "`count` is PROTECTED in module `counters` and cannot be assigned outside it"),
            ("table(2) = 1.0", "`table` is PROTECTED in module `counters` and cannot be assigned outside it"),
            ("last%a = 3", "`last` is PROTECTED in module `counters` and cannot be assigned outside it"),
            ("read (*, *) count", "cannot be read into outside it"),
            ("read (*, *, iostat=count) k", "cannot be read into outside it"),
            ("do count = 1, 2\n  end do", "cannot be used as a DO variable outside it"),
            ("call set(count)", "cannot be passed to an INTENT(OUT) or INTENT(INOUT) dummy of `set` outside it"),
            ("nullify(current)", "`current` is PROTECTED in module `counters` and cannot be pointer associated or nullified outside it"),
            ("allocate(sizes(3))", "`sizes` is PROTECTED in module `counters` and cannot be allocated or deallocated outside it"),
        ]
        for statement, message in cases:
            with self.subTest(statement=statement):
                self.reject(MODULE + program.format(statement), message)
        # Through a module that uses the variable's module, and renamed.
        self.reject(MODULE + """module reports
  use counters, only: total => count
contains
  subroutine reset()
    total = 0
  end subroutine
end module
program p
end program
""", "`total` is PROTECTED in module `counters` and cannot be assigned outside it")


if __name__ == "__main__":
    unittest.main()
