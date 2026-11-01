"""Module POINTER variables: their association is shared by every scope that
uses the module. Pointers start disassociated (with or without `=> null()`),
are associated in module procedures and in the main program, to module and
main-program targets and to array sections, and are tested, dereferenced
and nullified elsewhere, also through another module.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_module_pointers.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class ModulePointers(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_shared_association(self):
        self.check("""module state
  implicit none
  integer, target :: slots(4) = [10, 20, 30, 40]
  integer, pointer :: current => null()
  real, pointer :: window(:) => null()
  real, target :: data(6)
contains
  subroutine select(i)
    integer, intent(in) :: i
    current => slots(i)
  end subroutine
  subroutine open_window(lo, hi)
    integer, intent(in) :: lo, hi
    window => data(lo:hi)
  end subroutine
  integer function peek()
    peek = -1
    if (associated(current)) peek = current
  end function
end module
module user
  use state
contains
  subroutine bump()
    if (associated(current)) current = current + 1
  end subroutine
end module
program p
  use state
  use user
  implicit none
  integer :: i
  print *, associated(current), peek()
  call select(2)
  print *, associated(current), current, peek()
  call bump()
  print *, slots, associated(current, slots(2))
  data = [(real(i), i = 1, 6)]
  call open_window(2, 4)
  print *, size(window), window, sum(window)
  window = 0
  print *, data
  nullify(current)
  print *, associated(current), peek()
end program
""", "F -1\nT 20 20\n10 21 30 40 T\n3 2.0 3.0 4.0 9.0\n1.0 0.0 0.0 0.0 5.0 6.0\nF -1\n")

    def test_main_program_targets(self):
        self.check("""module m
  integer, pointer :: q
end module
program p
  use m
  integer, target :: t = 3
  nullify(q)
  if (associated(q)) q = 5
  print *, associated(q)
  q => t
  q = 7
  print *, associated(q), t, q
end program
""", "F\nT 7 7\n")


if __name__ == "__main__":
    unittest.main()
