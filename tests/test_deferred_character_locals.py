"""Deferred-length CHARACTER scalars local to module, external and internal
procedures: assignment, growth by concatenation, DEALLOCATE, ALLOCATED and
ALLOCATE(CHARACTER(LEN=n) :: s) with a runtime length. Module variables and
host variables of internal procedures, which live in module storage, and
deferred-length actual arguments of assumed-length dummies.

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

    def test_host_variables(self):
        # A main program's variable, and a nonrecursive procedure's, which
        # starts unallocated on each call.
        self.check("""program p
  integer :: k
  character(len=:), allocatable :: s
  s = 'a'
  call show
  call grow
  print '(a,i3)', s, len(s)
  do k = 1, 3
    call work(k)
  end do
contains
  subroutine show
    print '(a)', s
  end subroutine
  subroutine grow
    s = s // 'bcd'
  end subroutine
end program
subroutine work(n)
  integer, intent(in) :: n
  character(len=:), allocatable :: line
  integer :: i
  print '(l2)', allocated(line)
  line = ''
  do i = 1, n
    call add(i)
  end do
  print '(a,i3)', line, len(line)
contains
  subroutine add(j)
    integer, intent(in) :: j
    character(len=8) :: t
    write(t, '(i0)') j
    line = line // trim(t) // ','
  end subroutine
end subroutine
""", "a\nabcd  4\n F\n1,  2\n F\n1,2,  4\n F\n1,2,3,  6\n")

    def test_module_variables(self):
        self.check("""module log_m
  implicit none
  character(len=:), allocatable :: buf, last
contains
  subroutine push(s)
    character(len=*), intent(in) :: s
    if (allocated(buf)) then
      buf = buf // ';' // s
    else
      buf = s
    end if
    last = s
  end subroutine
  subroutine reset(n)
    integer, intent(in) :: n
    if (allocated(buf)) deallocate(buf)
    allocate(character(len=n) :: buf)
    buf(1:1) = '#'
    buf(2:n-1) = repeat('-', n - 2)
    buf(n:n) = '$'
  end subroutine
  integer function count_sep()
    integer :: i
    count_sep = 0
    do i = 1, len(buf)
      if (buf(i:i) == ';') count_sep = count_sep + 1
    end do
  end function
end module log_m
program main
  use log_m
  implicit none
  print '(l2)', allocated(buf)
  call push('alpha')
  call push('beta')
  call push('gamma')
  print '(a,1x,i0,1x,i0)', buf, len(buf), count_sep()
  print '(a,1x,a)', buf(7:10), last
  call show(buf)
  print '(l2,i3)', buf == 'alpha;beta;gamma', index(buf, 'beta')
  call reset(4)
  print '(3a,i2)', '[', buf, ']', len(buf)
  call twice(last)
  print '(a)', last
  deallocate(buf)
  print '(l2)', allocated(buf)
contains
  subroutine show(x)
    character(len=*), intent(in) :: x
    print '(3a)', '<', x, '>'
  end subroutine
  subroutine twice(x)
    character(len=*), intent(inout) :: x
    x = x(3:) // x(1:2)
  end subroutine
end program
""", " F\nalpha;beta;gamma 16 2\nbeta gamma\n<alpha;beta;gamma>\n T  7\n[#--$] 4\nmmaga\n F\n")

    def test_actual_arguments_and_comparisons(self):
        self.check("""subroutine ext(x)
  character(len=*), intent(in) :: x
  print '(3a,i3)', '<', x, '>', len(x)
end subroutine
program main
  implicit none
  character(len=:), allocatable :: buf
  integer :: i, c
  interface
    subroutine ext(x)
      character(len=*), intent(in) :: x
    end subroutine
  end interface
  buf = 'a;b;c'
  buf = buf // ';'
  call ext(buf)
  call ext(buf // '!')
  c = 0
  do i = 1, len(buf)
    if (buf(i:i) == ';') c = c + 1
  end do
  print '(i0,2l2)', c, buf == 'a;b;c;', buf(1:3) < 'a;c'
end program
""", "<a;b;c;>  6\n<a;b;c;!>  7\n3 T T\n")


if __name__ == "__main__":
    unittest.main()
