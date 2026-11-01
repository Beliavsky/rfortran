"""Submodules (Fortran 2008) compiled together with their ancestor module:
separate module procedures in both body forms, generic interfaces, nested
submodules, submodule-private entities, and the diagnostics for interfaces
and bodies that do not match. Programs run in the interpreter, at -O0 and
-O2, and with gfortran when it is installed.

Run after cargo build --bin rfort: python tests/test_submodules.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import COMPILER

SHAPES = '''module shapes
  implicit none
  private
  public :: circle, area, describe, scale_all, count_calls, total, fact, twice
  type :: circle
    real :: r = 1.0
  end type circle
  integer :: count_calls = 0
  real, parameter :: pi = 3.14159265
  interface area
    module pure function circle_area(c) result(a)
      type(circle), intent(in) :: c
      real :: a
    end function circle_area
  end interface area
  interface
    module subroutine describe(c, text)
      type(circle), intent(in) :: c
      character(len=*), intent(out) :: text
    end subroutine describe
    module subroutine scale_all(cs, factor)
      type(circle), intent(inout) :: cs(:)
      real, intent(in) :: factor
    end subroutine
    recursive module function fact(n) result(f)
      integer, intent(in) :: n
      integer :: f
    end function fact
    module function twice(x)
      real, intent(in) :: x
      real :: twice
    end function twice
  end interface
contains
  real function total(cs)
    type(circle), intent(in) :: cs(:)
    integer :: i
    total = 0
    do i = 1, size(cs)
      total = total + area(cs(i))
    end do
  end function total
  ! A separate module procedure may have its body in the module itself.
  module procedure twice
    twice = 2 * x
  end procedure twice
end module shapes

submodule (shapes) shapes_area
  implicit none
  real :: last = -1.0
contains
  module procedure circle_area
    a = pi * c%r**2
  end procedure circle_area
  module subroutine describe(c, text)
    type(circle), intent(in) :: c
    character(len=*), intent(out) :: text
    count_calls = count_calls + 1
    last = c%r
    write (text, '(a,f0.2,a,f0.3)') 'r=', c%r, ' area=', circle_area(c)
  end subroutine describe
end submodule shapes_area

submodule (shapes:shapes_area) shapes_more
  implicit none
  interface
    module function grow(r, factor) result(g)
      real, intent(in) :: r, factor
      real :: g
    end function
  end interface
contains
  module function grow(r, factor) result(g)
    real, intent(in) :: r, factor
    real :: g
    g = r * factor
  end function grow
  module procedure scale_all
    integer :: i
    do i = 1, size(cs)
      cs(i)%r = grow(cs(i)%r, factor)
    end do
    count_calls = count_calls + helper()
  end procedure
  integer function helper()
    helper = 10
  end function helper
  recursive module function fact(n) result(f)
    integer, intent(in) :: n
    integer :: f
    if (n <= 1) then
      f = 1
    else
      f = n * fact(n - 1)
    end if
  end function fact
end submodule shapes_more

program main
  use shapes
  implicit none
  type(circle) :: cs(2)
  character(len=40) :: text
  ! The submodules' `last` and `helper` are private to them.
  real :: last
  integer :: helper
  last = 5.0
  helper = 7
  cs(2)%r = 2.0
  call describe(cs(2), text)
  print '(a)', trim(text)
  print '(f0.3)', total(cs)
  call scale_all(cs, 3.0)
  print '(2f8.3)', cs%r
  print '(f0.3)', area(cs(1))
  print '(i0,1x,i0)', count_calls, fact(5)
  print '(f0.1,1x,f0.1,1x,i0)', twice(1.5), last, helper
end program main
'''

INTERFACE = '''module m
  implicit none
  interface
    module subroutine s(x)
      integer, intent(in) :: x
    end subroutine
  end interface
end module m
'''


class Submodules(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_separate_procedures_generics_and_nested_submodules(self):
        self.check(SHAPES, 'r=2.00 area=12.566\n15.708\n   3.000   6.000\n28.274\n11 120\n3.0 5.0 7\n')

    def test_procedure_body_with_interface_in_the_same_submodule(self):
        # gfortran rejects a MODULE PROCEDURE body whose interface is in the
        # same submodule, so there is no reference run.
        self.check('''module m
  implicit none
  interface
    module subroutine show()
    end subroutine
  end interface
end module m
submodule (m) impl
  implicit none
  interface
    module function grow(r) result(g)
      real, intent(in) :: r
      real :: g
    end function
  end interface
contains
  module procedure grow
    g = 2 * r
  end procedure grow
  module procedure show
    print '(f0.1)', grow(1.5)
  end procedure show
end submodule impl
program main
  use m
  call show()
end program main
''', '3.0\n', reference=False)

    def diagnostics(self, program, flags=()):
        with tempfile.TemporaryDirectory(prefix='submodule-check-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text(program, encoding='utf-8')
            result = subprocess.run([COMPILER, *flags, 'check', source], cwd=tmp,
                                    capture_output=True, text=True, timeout=30)
            return result.returncode, result.stderr

    def test_diagnostics(self):
        body = '\nsubmodule (m) b\ncontains\n{}\nend submodule b\n'
        cases = [
            (INTERFACE, 'separate module procedure `s` has no body', 'example.f90:4:23'),
            (INTERFACE + body.format('module subroutine s(y)\ninteger, intent(in) :: y\nend subroutine'),
             'dummy arguments (y) of separate module procedure `s` differ from its interface (x)', 'example.f90:12:19'),
            (INTERFACE + body.format('module function s(x)\ninteger :: x, s\nend function'),
             'separate module procedure `s` is a function but its interface declares a subroutine', 'example.f90:12:17'),
            (INTERFACE + body.format('module procedure s\nend procedure\nmodule procedure t\nend procedure'),
             'separate module procedure `t` has no interface in module `m`', 'example.f90:14:18'),
            (INTERFACE + body.format('module procedure s\nend procedure\nmodule procedure s\nend procedure'),
             'separate module procedure `s` has more than one body', 'example.f90:14:18'),
            ('submodule (nowhere) b\nend submodule\n',
             'its ancestor module `nowhere` must be in the same compilation', 'example.f90:1:12'),
            ('module m\nend module\nsubmodule (m:q) b\nend submodule\n',
             'its parent submodule `m:q` must come before it', 'example.f90:3:14'),
            ('module m\ninteger :: k\nend module\nsubmodule (m) b\nreal :: k\nend submodule\n',
             '`k` in submodule `b` is also declared in module `m`', 'example.f90:5:9'),
        ]
        for program, message, location in cases:
            with self.subTest(message=message):
                code, stderr = self.diagnostics(program)
                self.assertNotEqual(code, 0)
                self.assertIn(message, stderr)
                self.assertIn(location, stderr)

    def test_files_compiled_together(self):
        files = {
            'a.f90': INTERFACE,
            'b.f90': 'submodule (m) m_impl\ncontains\n  module procedure s\n    print *, x + 1\n  end procedure\nend submodule\n',
            'c.f90': 'program p\n  use m\n  call s(2)\nend program\n',
        }
        with tempfile.TemporaryDirectory(prefix='submodule-files-') as tmp:
            directory = Path(tmp)
            for name, text in files.items():
                (directory / name).write_text(text, encoding='utf-8')
            exe = directory / 'p.exe'
            result = subprocess.run([COMPILER, 'build', *files, '-o', exe], cwd=directory,
                                    capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.stdout.split(), ['3'])
            # The module alone has no body for its separate procedure.
            result = subprocess.run([COMPILER, 'check', 'a.f90'], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('separate compilation of submodules is not yet supported', result.stderr)


if __name__ == '__main__':
    unittest.main()
