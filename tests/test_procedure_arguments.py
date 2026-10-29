"""Dummy procedure arguments and procedure pointers.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_procedure_arguments.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class ProcedureArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_interface_dummy_function_in_module(self):
        self.check("""module quad
implicit none
contains
real(8) function sq(x)
  real(8), intent(in) :: x
  sq = x*x
end function sq
real(8) function cube(x)
  real(8), intent(in) :: x
  cube = x**3
end function cube
real(8) function integrate(f, a, b, n)
  interface
    real(8) function f(x)
      real(8), intent(in) :: x
    end function f
  end interface
  real(8), intent(in) :: a, b
  integer, intent(in) :: n
  integer :: i
  real(8) :: h
  h = (b - a)/n
  integrate = 0.5d0*(f(a) + f(b))
  do i = 1, n - 1
    integrate = integrate + f(a + i*h)
  end do
  integrate = integrate*h
end function integrate
end module quad
program p1
use quad
implicit none
print '(2f12.8)', integrate(sq, 0d0, 1d0, 1000), integrate(cube, 0d0, 2d0, 1000)
print *, integrate(sq, 0d0, 1d0, 1000)
end program p1
""", '  0.33333350  4.00000400\n0.3333335\n')

    def test_external_subroutine_dummy(self):
        self.check("""program p2
implicit none
external :: show
call apply(show, 3)
contains
subroutine apply(s, n)
  external :: s
  integer, intent(in) :: n
  call s(n)
  call s(n + 1)
end subroutine apply
end program p2
subroutine show(k)
  integer, intent(in) :: k
  print *, 'k =', k
end subroutine show
""", 'k = 3\nk = 4\n')

    def test_abstract_interface_procedure_dummy(self):
        self.check("""module ops
implicit none
abstract interface
  function binop(a, b) result(c)
    integer, intent(in) :: a, b
    integer :: c
  end function binop
end interface
contains
integer function add(a, b)
  integer, intent(in) :: a, b
  add = a + b
end function add
integer function mul(a, b)
  integer, intent(in) :: a, b
  mul = a*b
end function mul
integer function fold(op, x)
  procedure(binop) :: op
  integer, intent(in) :: x(:)
  integer :: i
  fold = x(1)
  do i = 2, size(x)
    fold = op(fold, x(i))
  end do
end function fold
end module ops
program p3
use ops
implicit none

print *, fold(add, [1, 2, 3, 4]), fold(mul, [1, 2, 3, 4])


end program p3
""", '10 24\n')

    def test_forwarding_keywords_and_internal_actuals(self):
        self.check("""module solvers
implicit none
contains
subroutine bisect(f, lo, hi, root)
  interface
    function f(x) result(y)
      real(8), intent(in) :: x
      real(8) :: y
    end function f
  end interface
  real(8), intent(in) :: lo, hi
  real(8), intent(out) :: root
  real(8) :: a, b, m
  integer :: k
  a = lo; b = hi
  do k = 1, 60
    m = 0.5d0*(a + b)
    if (f(a)*f(m) <= 0d0) then
      b = m
    else
      a = m
    end if
  end do
  root = m
end subroutine bisect
subroutine report(g, lo, hi)
  real(8), external :: g
  real(8), intent(in) :: lo, hi
  real(8) :: r
  call bisect(g, lo, hi, r)
  print '(a,f12.8)', 'root = ', r
end subroutine report
end module solvers
program p4
use solvers
implicit none
real(8) :: r
call bisect(f=quad, lo=0d0, hi=2d0, root=r)
print '(f12.8)', r
call report(cosine, 0d0, 3d0)
call report(quad, 0d0, 2d0)
contains
real(8) function quad(x)
  real(8), intent(in) :: x
  quad = x*x - 2d0
end function quad
real(8) function cosine(x)
  real(8), intent(in) :: x
  cosine = cos(x)
end function cosine
end program p4
""", '  1.41421356\nroot =   1.57079633\nroot =   1.41421356\n')

    def test_procedure_pointers(self):
        self.check("""module ops
implicit none
abstract interface
  function binop(a, b) result(c)
    integer, intent(in) :: a, b
    integer :: c
  end function binop
  subroutine action(n)
    integer, intent(in) :: n
  end subroutine action
end interface
contains
integer function add(a, b)
  integer, intent(in) :: a, b
  add = a + b
end function add
integer function mul(a, b)
  integer, intent(in) :: a, b
  mul = a*b
end function mul
subroutine shout(n)
  integer, intent(in) :: n
  print *, 'shout', n
end subroutine shout
end module ops
program p5
use ops
implicit none
procedure(binop), pointer :: p => null()
procedure(action), pointer :: q
integer :: i
print *, associated(p)
p => add
print *, associated(p), associated(p, add), associated(p, mul), p(6, 7)
do i = 1, 2
  if (i == 2) p => mul
  print *, i, p(i, 10)
end do
q => shout
call q(5)
nullify(p)
print *, associated(p)
end program p5
""", 'F\nT T F 13\n1 11\n2 20\nshout 5\nF\n')

    def test_use_only_of_private_module_with_external_dummy(self):
        # From nswc_mod.f90: QAGS takes an EXTERNAL function and is named in
        # USE ONLY of a module that is PRIVATE by default.
        self.check("""module m
implicit none
private
public :: qags
contains
subroutine qags(f, a, b, result, ier)
real, external :: f
real, intent(in) :: a, b
real, intent(out) :: result
integer, intent(out) :: ier
result = (f(a) + f(b))*(b - a)/2
ier = 0
end subroutine qags
end module m
program p
use m, only: qags
implicit none
real, external :: square
real :: r
integer :: ier
call qags(square, 0., 1., r, ier)
print *, r, ier
end program
real function square(x)
real, intent(in) :: x
square = x*x
end function
""", " 0.5 0\n")

    def test_invalid_actuals_are_diagnosed(self):
        self.reject("""module m
implicit none
contains
subroutine apply(s)
external :: s
call s()
end subroutine apply
end module m
program main
use m
implicit none
integer :: k
call apply(k)
end program
""", "must be a procedure")

if __name__ == "__main__":
    unittest.main()
