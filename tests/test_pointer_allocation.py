"""ALLOCATE and DEALLOCATE of POINTERs, pointer bounds remapping, and
derived-type and CHARACTER pointers. Programs run in the interpreter, at
-O0 and -O2, and with gfortran when it is installed.

Run after cargo build --bin rfort: python tests/test_pointer_allocation.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class PointerAllocation(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_allocate_and_deallocate(self):
        self.check('''module store
  implicit none
  real, pointer :: shared(:) => null()
contains
  subroutine grow(n)
    integer, intent(in) :: n
    allocate(shared(0:n-1))
    shared = 0.5
  end subroutine grow
end module store
program main
  use store
  implicit none
  integer, pointer :: p(:), q(:), r(:,:), s
  logical, pointer :: flag
  double precision, pointer :: d(:)
  integer :: i, ios
  character(len=80) :: msg
  call grow(3)
  print '(3f5.1,2i3)', shared, lbound(shared), ubound(shared)
  allocate(p(-2:2), stat=ios)
  print '(a,i0)', 'stat ', ios
  p = [(i*i, i=-2,2)]
  q => p(::2)
  print '(3i4)', q
  print '(l2)', associated(q, p), associated(p)
  q => p
  print '(l2)', associated(q, p)
  allocate(r(2,3))
  r = reshape([(i, i=1,6)], [2,3])
  print '(3i3)', r(2,:), shape(r)
  print '(i0)', sum(r)
  allocate(s)
  s = 7
  allocate(flag)
  flag = .true.
  print '(i2,l2)', s, flag
  allocate(d(4), source=1.25d0)
  print '(4f6.2)', d
  deallocate(d, flag, r, s)
  print '(3l2)', associated(d), associated(flag), associated(r)
  ! A second ALLOCATE gives new storage; q keeps the old storage.
  allocate(p(2))
  p = 9
  print '(5i4)', q
  print '(2i4)', p
  deallocate(q, stat=ios, errmsg=msg)
  print '(a,i0)', 'stat ', ios
  deallocate(p)
end program main
''', '''  0.5  0.5  0.5  0  2
stat 0
   4   0   4
 F
 T
 T
  2  4  6
  2  3
21
 7 T
  1.25  1.25  1.25  1.25
 F F F
   4   1   0   1   4
   9   9
stat 0
''')

    def test_allocation_in_procedures_and_loops(self):
        self.check('''program main
  implicit none
  integer(1), pointer :: b(:)
  real, pointer :: x(:,:,:), row(:)
  integer, pointer :: e(:), keep(:)
  logical, pointer :: m(:)
  integer :: i, total
  call make(b, 3)
  print '(3i4)', b
  allocate(x(2,2,2))
  x = 1.0
  x(2,1,2) = 5.0
  row => x(2,1,:)
  print '(2f5.1,i3)', row, size(x)
  allocate(e(1:0))
  print '(i0,1x,l1)', size(e), associated(e)
  deallocate(e)
  total = 0
  do i = 1, 3
    allocate(e(i))
    e = i
    total = total + sum(e)
    if (i == 2) keep => e
    if (i /= 2) deallocate(e)
  end do
  print '(i0,1x,2i3)', total, keep
  allocate(m(3))
  m = [.true., .false., .true.]
  print '(i0,1x,l1)', count(m), any(m)
  deallocate(x, keep, m, b)
contains
  subroutine make(p, n)
    integer(1), pointer, intent(out) :: p(:)
    integer, intent(in) :: n
    integer :: k
    allocate(p(n))
    do k = 1, n
      p(k) = int(-k, 1)
    end do
  end subroutine make
end program main
''', '''  -1  -2  -3
  1.0  5.0  8
0 T
14   2  2
2 T
''')

    def test_deallocate_errors(self):
        for body, message in [
            ('integer, target :: t(2)\ninteger, pointer :: p(:)\np => t\ndeallocate(p)',
             'pointer `p` in DEALLOCATE is not associated with a whole allocated target'),
            ('integer, pointer :: p(:)\ndeallocate(p)', 'DEALLOCATE of unassociated pointer `p`'),
            ('integer, pointer :: p(:), q(:)\nallocate(p(4))\nq => p(2:3)\ndeallocate(q)',
             'pointer `q` in DEALLOCATE is not associated with a whole allocated target'),
            ('integer, pointer :: p(:)\nallocate(p(3))\np(1) = 1\nprint *, p(2)',
             'read of uninitialized variable `p`: element 2'),
        ]:
            with self.subTest(body=body):
                self.check(f'program main\nimplicit none\n{body}\nend program main\n', error=message)
        self.reject('program main\ninteger, pointer :: p(:,:)\nallocate(p(3))\nend program main\n',
                    'ALLOCATE of POINTER `p` requires 2 bounds')

    def test_bounds_remapping(self):
        self.check('''program main
  implicit none
  integer, target :: t(6) = [1,2,3,4,5,6]
  real, target :: a(2,3)
  integer, pointer :: p(:), m(:,:)
  real, pointer :: v(:), w(:,:), q(:)
  integer :: i
  p(0:) => t
  print '(7i3)', p, lbound(p), ubound(p)
  m(1:2, 1:3) => t
  print '(6i3)', m(2,:), shape(m)
  m(0:1, 5:6) => t(3:6)
  print '(6i3)', m(1,6), lbound(m), ubound(m)
  p(1:3) => t(::2)
  print '(3i3)', p
  a = reshape([(real(i), i=1,6)], [2,3])
  v(1:6) => a
  print '(6f4.0)', v
  w(1:3,1:2) => a
  print '(3f4.0)', w(:,2)
  allocate(q(4))
  q = [10., 20., 30., 40.]
  q(-1:) => q
  print '(4f5.0,2i3)', q, lbound(q), ubound(q)
  w(0:1,0:1) => q
  print '(f5.0)', w(1,1)
  deallocate(q)
  print '(l2)', associated(q)
end program main
''', '''  1  2  3  4  5  6  0
  5
  2  4  6  2  3
  6  0  5  1  6
  1  3  5
  1.  2.  3.  4.  5.  6.
  4.  5.  6.
  10.  20.  30.  40. -1  2
  40.
 F
''')

    def test_bounds_remapping_errors(self):
        self.check('program main\nreal, target :: a(4,3)\nreal, pointer :: v(:)\nv(1:6) => a(1:2,:)\nprint *, size(v)\nend program main\n',
                   error='bounds remapping of pointer `v` requires a rank-one or contiguous target')
        self.check('program main\nreal, target :: a(4)\nreal, pointer :: v(:,:)\nv(1:2,1:3) => a\nprint *, size(v)\nend program main\n',
                   error='pointer `v` bounds remapping needs more elements than its target has')

    def test_derived_type_pointers(self):
        self.check('''module shapes
  implicit none
  type point
    real :: x = 0., y = 0.
    integer :: tag(2) = [0, 0]
  contains
    procedure :: norm2 => point_norm2
  end type
  type holder
    type(point), pointer :: p => null()
  end type
  type(point), pointer :: origin => null()
contains
  real function point_norm2(self)
    class(point), intent(in) :: self
    point_norm2 = self%x**2 + self%y**2
  end function
  subroutine shift(p, dx)
    type(point), pointer, intent(inout) :: p
    real, intent(in) :: dx
    p%x = p%x + dx
  end subroutine
  subroutine make(p)
    type(point), pointer, intent(out) :: p
    allocate(p)
    p = point(3., 4., [1, 2])
  end subroutine
end module shapes
program main
  use shapes
  implicit none
  type(point), target :: a, arr(3)
  type(point), pointer :: p, q
  type(point) :: copy
  type(holder) :: h
  integer :: i
  a = point(1., 2., [5, 6])
  p => a
  call shift(p, 10.)
  print '(2f6.1,2i3)', a%x, a%y, a%tag
  call make(q)
  print '(f6.1,l2,l2)', q%norm2(), associated(q), associated(q, a)
  copy = q
  q%x = -1.
  print '(2f6.1)', copy%x, q%x
  do i = 1, 3
    arr(i)%x = real(i)
  end do
  p => arr(3)
  h%p => arr(2)
  print '(2f6.1)', p%x, h%p%x
  allocate(origin)
  origin%tag(2) = 9
  print '(2i3)', origin%tag
  deallocate(q, origin)
  print '(2l2)', associated(q), associated(origin)
  nullify(p)
  print '(l2)', associated(p)
end program main
''', '''  11.0   2.0  5  6
  25.0 T F
   3.0  -1.0
   3.0   2.0
  0  9
 F F
 F
''')

    def test_character_pointers(self):
        self.check('''program main
  implicit none
  character(len=5), target :: s, arr(3)
  character(len=5), pointer :: c, ca(:), d
  integer :: i
  s = 'hello'
  c => s
  c(1:1) = 'J'
  arr = ['ab   ', 'cd   ', 'ef   ']
  ca => arr(2:3)
  ca(2) = 'xy'
  ca(1)(2:2) = 'Z'
  print '(a,1x,i0,1x,a,1x,a)', s, len(c), ca(2)(1:1), arr(3)
  print '(3a6)', arr
  print '(a)', trim(c) // '!'
  allocate(d)
  d = 'new'
  print '(a,l2,l2)', d, associated(d), associated(c, s)
  do i = 1, size(ca)
    print '(i0,1x,a)', i, ca(i)
  end do
  if (c == 'Jello') print '(a)', 'equal'
  print '(i0)', index(c, 'l')
  print *, c, ca(1), 'x', c(2:3), ca(2)(1:1), ca
  deallocate(d)
  nullify(c)
  print '(l2)', associated(c)
end program main
''', '''Jello 5 x xy
 ab    cZ    xy
Jello!
new   T T
1 cZ
2 xy
equal
3
 JellocZ   xelxcZ   xy
 F
''')

    def test_character_pointers_in_modules_and_procedures(self):
        self.check('''module names
  implicit none
  character(len=4), pointer :: current => null()
  character(len=4), target :: table(3) = ['ab  ', 'cd  ', 'ef  ']
contains
  subroutine pick(i)
    integer, intent(in) :: i
    current => table(i)
  end subroutine
  subroutine fill(p, text)
    character(len=4), pointer, intent(inout) :: p
    character(len=*), intent(in) :: text
    p = text
  end subroutine
end module names
program main
  use names
  implicit none
  character(len=4), pointer :: list(:), one
  character, pointer :: f
  call pick(2)
  print '(a)', current
  call fill(current, 'XY')
  print '(3(a,"|"))', table
  allocate(list(2))
  list(1) = 'one'
  list(2) = 'two'
  print '(a,1x,a)', list(1), current
  one => list(2)
  read(*,'(a)') one
  print '(a)', list(2)
  allocate(f)
  f = 'q'
  print '(a,1x,2a)', f, list
  deallocate(list, f)
end program main
''', '''cd
ab  |XY  |ef  |
one  XY
zz
q one zz
''', data='zz\n')

    def test_unsupported_character_pointers(self):
        self.reject('program main\ncharacter(len=:), pointer :: d\nallocate(character(len=3) :: d)\nend program main\n',
                    'deferred-length CHARACTER pointer `d` is not yet supported')
        self.reject('program main\ncharacter(len=3), target :: s\ncharacter(len=3), pointer :: c\nc => s(1:2)\nend program main\n',
                    'pointer association with a substring of `s` is not yet supported')

    def test_formatted_read_into_allocatable_character_element(self):
        self.check('''program main
  implicit none
  character(len=4), allocatable :: names(:)
  integer :: k
  allocate(names(0:3))
  names = 'none'
  k = 2
  read(*,'(a)') names(k)
  read(*,'(a)') names(0)
  print '(4a5)', names
end program main
''', ' cd   none zz   none\n', data='zz\ncd\n')

    def test_statement_function_names_of_host_and_module_arrays(self):
        # pool(h) = ... assigns an element of a module array; it is not a
        # statement function definition.
        self.check('''module m
  implicit none
  real, allocatable :: pool(:)
contains
  subroutine bump(h, dx)
    integer, intent(in) :: h
    real, intent(in) :: dx
    pool(h) = pool(h) + dx
  end subroutine
end module m
program main
  use m
  implicit none
  real :: host(3)
  integer :: a
  allocate(pool(0:4))
  pool = 0
  pool(2) = 1.
  a = 2
  call bump(a, 10.)
  host = 0
  call set(3)
  print '(f5.1,3f4.0)', pool(a), host
contains
  subroutine set(i)
    integer, intent(in) :: i
    host(i) = 4.
  end subroutine
end program main
''', ' 11.0  0.  0.  4.\n')


if __name__ == '__main__':
    unittest.main()
