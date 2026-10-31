"""ENTRY statements: subroutine and function entries with different dummy
arguments, RESULT clauses, results of the same type sharing storage, SAVE
and DATA variables shared by all entry points, array dummies whose bounds are
dummies of the entry, and entries of module procedures. Also SEQUENCE in
type definitions, RESULT variables typed implicitly, and RESHAPE with
SHAPE(x) of an explicit-shape array.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_entry_statements.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class EntryStatements(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_subroutine_entries_share_saved_state(self):
        self.check("""subroutine counter(n)
  integer n, total, step
  save total
  data step /1/
  total = n
  return
  entry bump
  total = total + step
  return
  entry report(n)
  n = total
  entry setstep(n)
  step = n
end subroutine

program p
  integer k
  call counter(10)
  call bump
  call bump
  call report(k)
  print *, k
  call setstep(5)
  call bump
  call report(k)
  print *, k
end program
""", "12\n17\n")

    def test_function_entries_and_results(self):
        self.check("""function area(w, h)
  real area, side, square, w, h, s
  area = w * h
  return
  entry square(s)
  square = s * s
  return
  entry side(w)
  side = w
end function

function f4(a) result(r)
  double precision a, b, r, s
  r = 15 + a
  return
  entry e4(b) result(s)
  s = 42 + b
end function

function f8() result(r)
  entry e8()
  e8 = 115
end function

program p
  real area, square, side, f8, e8
  double precision f4, e4
  print *, area(2., 3.), square(4.), side(1.5)
  print *, f4(1d0), e4(1d0)
  print *, f8(), e8()
end program
""", "6.0 16.0 1.5\n16.0 43.0\n115.0 115.0\n")

    def test_array_dummies(self):
        # x is not a dummy of total, so its bound n need not be defined there.
        self.check("""subroutine store(x, n)
  integer n, i, k
  real x(n), keep(10), t
  save keep, k
  do i = 1, n
    keep(i) = x(i)
  end do
  k = n
  return
  entry total(t)
  t = sum(keep(1:k))
end subroutine

program p
  real v(3), t
  v = [1., 2., 4.]
  call store(v, 3)
  call total(t)
  print *, t
end program
""", "7.0\n")

    def test_module_procedure_entries(self):
        self.check("""module m
contains
  subroutine on(v)
    integer v, state
    save state
    data state /0/
    state = v
    return
    entry off
    state = 0
    return
    entry show
    print *, state
  end subroutine
end module

program p
  use m
  call on(3)
  call show
  call off
  call show
end program
""", "3\n0\n")

    def test_sequence_and_shape(self):
        self.check("""program p
  type pair
    sequence
    integer i, j
  end type
  type(pair) q
  integer b(3,2), c(3,2)
  q = pair(1, 2)
  b = 1
  c = reshape([(i, i = 1, 6)], shape(b))
  print *, q%i + q%j, c(3,2), c(1,2)
end program
""", "3 6 4\n")

    def test_rejections(self):
        self.reject("""program p
  integer i
  entry q
end program
""", "ENTRY is allowed only in a subroutine or function")


if __name__ == "__main__":
    unittest.main()
