"""Parameterized derived types with constant type parameters: KIND and LEN
parameters with defaults, positional and keyword values, values from named
constants, KIND, SELECTED_REAL_KIND and module constants; arrays of such
objects and procedure arguments; type parameter inquiries, also through
components and of CHARACTER components; types in modules, used with ONLY and
renaming, and parameterized types of the same name in two modules; extension
of parameterized types; structure constructors with type parameter values;
SELECT TYPE guards; and the runtime LEN parameters not yet supported.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_parameterized_types.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class ParameterizedTypes(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_kind_and_len_parameters(self):
        self.check("""module m
  implicit none
  integer, parameter :: dp = kind(1d0)
  type :: matrix(k, n)
    integer, kind :: k = 4
    integer, len :: n
    real(kind=k) :: a(n, n)
    character(len=n) :: label
  end type
contains
  subroutine show(x)
    type(matrix(dp, 3)), intent(in) :: x
    print *, x%k, x%n, kind(x%a), size(x%a), sum(x%a), len(x%label)
  end subroutine
end module
program p
  use m
  implicit none
  type(matrix(dp, 3)) :: x
  type(matrix(n=2)) :: y
  type(matrix(k=8, n=3)) :: z(2)
  real(kind=x%k) :: w
  x%a = 1
  x%label = 'abc'
  call show(x)
  y%a = 2.5
  print *, y%k, y%n, kind(y%a), sum(y%a)
  z(2)%a = 3
  z(2)%label = 'q'
  print *, z(2)%n, sum(z(2)%a), kind(w)
  call show(z(2))
end program
""", "8 3 8 9 9.0 3\n4 2 4 10.0\n3 27.0 8\n8 3 8 9 27.0 3\n")

    def test_values_from_constants_and_intrinsics(self):
        self.check("""module kinds
  integer, parameter :: sp = selected_real_kind(6, 37)
  integer, parameter :: dp = selected_real_kind(15, 307)
  integer, parameter :: n0 = 4
end module
program p
  use kinds
  type t(k, m)
    integer, kind :: k
    integer, len :: m
    real(k) :: v(m)
    integer(kind=k) :: big
  end type
  type(t(sp, n0)) :: a
  type(t(dp, 2*n0)) :: b
  type(t(kind(1_8), 1)) :: c
  print *, kind(a%v), size(a%v), kind(b%v), size(b%v), kind(c%big), b%m
end program
""", "4 4 8 8 8 8\n")

    def test_modules_only_rename_and_same_names(self):
        self.check("""module m1
  type t(n)
    integer, len :: n
    integer :: a(n)
  end type
end module
module m2
  type t(k)
    integer, kind :: k
    real(k) :: x
  end type
end module
program p
  use m1, only: vec => t
  use m2
  type(vec(3)) :: v
  type(t(8)) :: r
  v%a = [1, 2, 3]
  r%x = 0.5d0
  print *, sum(v%a), v%n, kind(r%x), r%k
end program
""", "6 3 8 8\n")

    def test_nested_components_and_character_inquiries(self):
        # gfortran 17 fails with an internal compiler error on this program.
        self.check("""program p
  type :: matrix(k, n)
    integer, kind :: k
    integer, len :: n
    real(k) :: a(n, n)
  end type
  type :: problem(n)
    integer, len :: n = 2
    type(matrix(kind(0.d0), n)) :: m
    character(len=n+1) :: c
  end type
  type(problem) :: q
  type(problem(4)) :: r
  integer, parameter :: lc = len(r%c)
  q%m%a = 1
  print *, q%n, q%m%n, q%m%k, sum(q%m%a)
  print *, lc, r%c%len, r%m%n, size(r%m%a)
end program
""", "2 2 8 4.0\n5 5 4 16\n", reference=False)

    def test_extension(self):
        self.check("""program p
  type :: base(k)
    integer, kind :: k = 4
    real(k) :: x
  end type
  type, extends(base) :: point(n)
    integer, len :: n
    integer :: tags(n)
  end type
  type(point(8, 3)) :: a
  type(point(n=2)) :: b
  a%x = 1.5d0
  a%tags = [1, 2, 3]
  b%x = 2.0
  print *, a%k, a%n, kind(a%x), sum(a%tags), b%k, kind(b%x), size(b%tags)
end program
""", "8 3 8 6 4 4 2\n")

    def test_constructors(self):
        self.check("""program p
  type pair(k)
    integer, kind :: k = 4
    integer(k) :: i, j
  end type
  type text(n)
    integer, len :: n = 3
    character(len=n) :: s
  end type
  type(pair(8)) :: a
  type(pair) :: b
  type(text(4)) :: c
  a = pair(8)(1, 2)
  b = pair(3, 4)
  c = text(4)('abcd')
  print *, a%i + a%j, kind(a%i), b%i * b%j, c%s, len(c%s)
end program
""", "3 8 12 abcd 4\n")

    def test_select_type(self):
        # A parameterized extension of a type without parameters.
        self.check("""program p
  type base
    integer :: id = 0
  end type
  type, extends(base) :: vec(k)
    integer, kind :: k
    real(k) :: x
  end type
  class(base), allocatable :: v
  type(vec(4)) :: a
  type(vec(8)) :: b
  integer i
  a%id = 1
  a%x = 1.5
  b%id = 2
  b%x = 2.5d0
  do i = 1, 2
    if (i == 1) then
      allocate(v, source=a)
    else
      allocate(v, source=b)
    end if
    select type (v)
    type is (vec(4))
      print *, 'single', v%id, v%x
    type is (vec(8))
      print *, 'double', v%id, v%x
    end select
    deallocate(v)
  end do
end program
""", "single 1 1.5\ndouble 2 2.5\n")

    def test_runtime_len_values(self):
        # LEN values known only at run time, assumed by dummies, and of
        # arrays of objects; CHARACTER and array components depend on them.
        self.check("""module m
  implicit none
  type :: vec(k, n)
    integer, kind :: k = 4
    integer, len :: n
    real(k) :: v(n)
    character(len=n) :: tag
  end type
contains
  subroutine show(x)
    type(vec(8, *)), intent(in) :: x
    print *, x%n, size(x%v), sum(x%v), len(x%tag), kind(x%v)
  end subroutine
  subroutine fill(x, value)
    type(vec(8, *)), intent(inout) :: x
    real(8), intent(in) :: value
    x%v = value
  end subroutine
end module
program p
  use m
  implicit none
  integer :: m1
  type(vec(8, :)), allocatable :: d
  type(vec(8, 2)) :: c
  m1 = 3
  call work(m1)
  allocate(vec(8, 5) :: d)
  call fill(d, 2d0)
  d%tag = 'abcde'
  call show(d)
  print *, d%n, d%tag
  c%v = [1d0, 2d0]
  c%tag = 'cc'
  call show(c)
contains
  subroutine work(n)
    integer, intent(in) :: n
    type(vec(8, n)) :: a
    type(vec(n=2*n)) :: b(2)
    a%v = 1
    a%tag = 'xyz'
    call show(a)
    b(2)%v = 4
    print *, b(2)%n, size(b(2)%v), sum(b(2)%v), kind(b(1)%v)
  end subroutine
end program
""", "3 3 3.0 3 8\n6 6 24.0 4\n5 5 10.0 5 8\n5 abcde\n2 2 3.0 2 8\n")

    def test_deferred_len_allocation(self):
        self.check("""module m
  implicit none
  type :: vec(n)
    integer, len :: n
    real :: v(n)
  end type
contains
  subroutine show(x)
    type(vec(*)), intent(in) :: x
    print *, x%n, size(x%v), sum(x%v)
  end subroutine
end module
program p
  use m
  implicit none
  type(vec(:)), allocatable :: d
  print *, allocated(d)
  allocate(vec(5) :: d)
  d%v = 2
  call show(d)
  print *, allocated(d), d%n
  deallocate(d)
  allocate(vec(n=2) :: d)
  d%v = [1., 3.]
  call show(d)
end program
""", "F\n5 5 10.0\nT 5\n2 2 4.0\n")

    def test_rejections(self):
        self.reject("""program p
  type :: vec(k)
    integer, len :: k = 3
    integer :: foo(k)
  end type
  type(vec) :: a(2)
  integer :: n
  n = 3
  call make(n)
contains
  subroutine make(m)
    integer :: m
    type(vec(m)) :: b(2)
    b%foo = 1
  end subroutine
end program
""", "`b%foo` of a whole array whose type has LEN parameters known only at run time is not yet supported")
        self.reject("""program p
  type t(k)
    integer, kind :: k
    real(k) :: x
  end type
  type(t) :: x
end program
""", "type parameter `k` of `t` has no default and needs a value")


if __name__ == "__main__":
    unittest.main()
