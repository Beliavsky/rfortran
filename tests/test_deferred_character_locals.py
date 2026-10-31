"""Deferred-length CHARACTER scalars local to module, external and internal
procedures: assignment, growth by concatenation, DEALLOCATE, ALLOCATED and
ALLOCATE(CHARACTER(LEN=n) :: s) with a runtime length.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_deferred_character_locals.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class DeferredCharacterLocals(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_procedure_locals(self):
        self.check("""module m
contains
  subroutine greet(n)
    integer n, i
    character(len=:), allocatable :: s
    s = 'hi'
    do i = 1, n
      s = s // '!'
    end do
    print *, len(s), s
    deallocate(s)
    print *, allocated(s)
    allocate(character(len=n) :: s)
    s = 'abc'
    print *, len(s), '[', s, ']'
  end subroutine
end module
program p
  use m
  call greet(3)
  call inner
  call outer(2)
contains
  subroutine inner
    character(len=:), allocatable :: t
    t = 'inner'
    t = t // t
    print *, t
  end subroutine
end program
subroutine outer(k)
  integer k
  character(len=:), allocatable :: u
  u = repeat('ab', k)
  print *, u, len(u)
end subroutine
""", "5 hi!!!\nF\n3 [abc]\ninnerinner\nabab 4\n")

    def test_host_variable_is_rejected(self):
        self.reject("""program p
  character(len=:), allocatable :: s
  s = 'x'
  call show
contains
  subroutine show
    print *, s
  end subroutine
end program
""", "host association for `s` currently supports")


if __name__ == "__main__":
    unittest.main()
