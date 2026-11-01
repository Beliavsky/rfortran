"""Procedure pointer components: NOPASS and PASS (also PASS(arg) on a later
dummy) components with abstract and procedure interfaces, functions and
subroutines, association in another module's procedures (callbacks),
structure constructors, ASSOCIATED with and without a target, NULLIFY,
copies with their objects, arrays of objects, components of nested and
extended types, internal procedures of the main program as targets, and the
error of a call through a disassociated component.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_procedure_pointer_components.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class ProcedurePointerComponents(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_nopass_functions(self):
        self.check("""module ops
  implicit none
  abstract interface
    real function unary(x)
      real, intent(in) :: x
    end function
  end interface
  type op
    character(len=8) :: name
    procedure(unary), pointer, nopass :: f => null()
  end type
contains
  real function twice(x)
    real, intent(in) :: x
    twice = 2 * x
  end function
  real function square(x)
    real, intent(in) :: x
    square = x * x
  end function
end module
program p
  use ops
  implicit none
  type(op) :: a, b
  a%name = 'twice'
  a%f => twice
  b = op('square', square)
  print *, a%f(3.0), b%f(3.0), associated(a%f), associated(b%f, square)
  nullify(a%f)
  print *, associated(a%f)
end program
""", "6.0 9.0 T T\nF\n")

    def test_pass_callbacks_and_arrays(self):
        self.check("""module shapes
  implicit none
  type :: shape
    real :: size = 1
    procedure(area_i), pointer :: area => null()
    procedure(scale_i), pointer, pass(s) :: scale => null()
    procedure(report_i), pointer :: report => null()
  end type
  abstract interface
    real function area_i(self)
      import shape
      class(shape), intent(in) :: self
    end function
    subroutine scale_i(factor, s)
      import shape
      real, intent(in) :: factor
      class(shape), intent(inout) :: s
    end subroutine
    subroutine report_i(self, label)
      import shape
      class(shape), intent(in) :: self
      character(len=*), intent(in) :: label
    end subroutine
  end interface
contains
  real function square_area(self)
    class(shape), intent(in) :: self
    square_area = self%size**2
  end function
  subroutine grow(factor, s)
    real, intent(in) :: factor
    class(shape), intent(inout) :: s
    s%size = s%size * factor
  end subroutine
end module
module callbacks
  use shapes
  implicit none
contains
  real function circle_area(self)
    class(shape), intent(in) :: self
    circle_area = 3.0 * self%size**2
  end function
  subroutine say(self, label)
    class(shape), intent(in) :: self
    character(len=*), intent(in) :: label
    print *, label, self%size
  end subroutine
  subroutine setup(s, round)
    type(shape), intent(inout) :: s
    logical, intent(in) :: round
    if (round) then
      s%area => circle_area
    else
      s%area => square_area
    end if
    s%scale => grow
    s%report => say
  end subroutine
end module
program p
  use shapes
  use callbacks
  implicit none
  type(shape) :: a, b, c(2)
  integer :: i
  call setup(a, .false.)
  call setup(b, .true.)
  a%size = 2
  b%size = 2
  print *, a%area(), b%area()
  call a%scale(1.5)
  call a%report('scaled')
  c(1) = a
  c(2) = b
  do i = 1, 2
    print *, i, c(i)%area()
    call c(i)%report("elem")
  end do
  print *, associated(c(2)%area, circle_area), associated(c(1)%area, circle_area)
end program
""", "4.0 12.0\nscaled 3.0\n1 9.0\nelem 3.0\n2 12.0\nelem 2.0\nT F\n")

    def test_nopass_subroutines_extension_and_nesting(self):
        self.check("""program p
  implicit none
  type :: handler
    integer :: calls = 0
    procedure(on_event), pointer, nopass :: notify => null()
    procedure(twice), pointer, nopass :: transform => null()
  end type
  type, extends(handler) :: named
    character(len=4) :: tag = 'none'
  end type
  type :: holder
    type(handler) :: inner
  end type
  type(named) :: n
  type(holder) :: h
  n%notify => shout
  n%transform => twice
  call n%notify(3)
  print *, n%transform(2.5), n%tag
  h%inner%notify => whisper
  call h%inner%notify(4)
  n%notify => whisper
  call n%notify(5)
  print *, associated(h%inner%transform)
contains
  subroutine on_event(k)
    integer, intent(in) :: k
  end subroutine
  real function twice(x)
    real, intent(in) :: x
    twice = 2 * x
  end function
  subroutine shout(k)
    integer, intent(in) :: k
    print *, 'SHOUT', k
  end subroutine
  subroutine whisper(k)
    integer, intent(in) :: k
    print *, 'whisper', k
  end subroutine
end program
""", "SHOUT 3\n5.0 none\nwhisper 4\nwhisper 5\nF\n")

    def test_local_pointers_and_pass_arguments(self):
        self.check("""module m
  implicit none
  type :: t
    integer :: i = 0
    procedure(foo), pointer, pass(y) :: foo => null()
    procedure(twice), pointer, nopass :: f => null()
  end type
contains
  subroutine foo(x, y)
    type(t), optional :: x
    class(t) :: y
    if (present(x)) then
      print *, 'foo', x%i, y%i
    else
      print *, 'foo', y%i
    end if
  end subroutine
  integer function twice(n)
    integer, intent(in) :: n
    twice = 2 * n
  end function
end module
program p
  use m
  implicit none
  type(t) :: t1, t2
  procedure(twice), pointer :: g
  t1%i = 4
  t2%i = 7
  t1%foo => foo
  t2%foo => t1%foo
  call t1%foo()
  call t2%foo(t1)
  t1%f => twice
  g => t1%f
  print *, g(5), associated(g, twice)
  t2%f => g
  print *, t2%f(6)
end program
""", "foo 4\nfoo 4 7\n10 T\n12\n")

    def test_disassociated_call(self):
        self.check("""program p
  implicit none
  type :: handler
    procedure(work), pointer, nopass :: run => null()
  end type
  type(handler) :: h
  call h%run(1)
contains
  subroutine work(k)
    integer, intent(in) :: k
    print *, k
  end subroutine
end program
""", error="call through disassociated procedure pointer component", reference=False)


if __name__ == "__main__":
    unittest.main()
