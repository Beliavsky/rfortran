"""Derived types: runtime ALLOCATE in procedures with derived dummies, output
of whole objects, PARAMETER and SAVE objects, module objects, array and nested
components in arrays of records, keyword constructors, generic bindings,
defined assignment and type extension.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_derived_type_features.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class DerivedTypeFeatures(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_runtime_allocate_in_procedures(self):
        """ALLOCATE with a runtime extent in a procedure with array or derived dummies,
        and a function with an ALLOCATABLE INTENT(IN) array dummy."""
        self.check("""module m
type :: t
  real, allocatable :: w(:)
end type
contains
subroutine s(p)
type(t), intent(inout) :: p
real, allocatable :: nk(:)
integer :: k
k = size(p%w)
allocate(nk(k))
nk = p%w
print *, nk
end subroutine
end module
program p
use m
type(t) :: a
allocate(a%w(3)); a%w = [1.,2.,1.]
call s(a)
end program
""", ' 1.0 2.0 1.0\n')
        self.check("""module m
contains
real function total(w)
  real, intent(in) :: w(:)
  total = sum(w)
end function
real function total2(w)
  real, allocatable, intent(in) :: w(:)
  total2 = sum(w)
end function
end module
program p
use m
real, allocatable :: a(:)
real :: x
integer :: n
call random_number(x)
n = 2 + int(x)
allocate(a(n)); a = 1
print *, total(a)
print *, total2(a)
end program
""", ' 2.0\n 2.0\n')

    def test_whole_object_output(self):
        """A derived object in an output list stands for its components."""
        self.check("""module m
type :: pt
  real :: x
  integer :: k
end type
type :: seg
  type(pt) :: a, b
  logical :: on = .true.
  character(len=3) :: tag = 'ab'
end type
end module
program p
use m
type(pt) :: a
type(seg) :: s
a = pt(1.5, 2)
s%a = a
s%b = pt(-1.0, 7)
print *, a
print *, 'seg', s, 9
write(*,*) a, s%b
write(*,'(f5.1,i3)') a
print '(2(f5.1,i3),l2,1x,a)', s
end program
""", ' 1.5 2\n seg 1.5 2 -1.0 7 T ab  9\n 1.5 2 -1.0 7\n  1.5  2\n  1.5  2 -1.0  7 T ab \n')
        self.check("""program p
type :: node
  integer :: id = 0
  real :: w = 1
end type
type :: graph
  integer :: n = 2
  type(node) :: root(2)
  real :: v(2) = [3., 4.]
end type
type(graph) :: g
g%root(2) = node(5, 2.5)
print *, g
end program
""", ' 2 0 1.0 5 2.5 3.0 4.0\n')

    def test_parameter_save_and_module_objects(self):
        """Derived PARAMETERs, SAVE, initialized and module objects, with PUBLIC/PRIVATE."""
        self.check("""module geo
implicit none
type :: pt
  real :: x = 0, y = 0
end type
type :: box
  type(pt) :: lo, hi = pt(1., 1.)
  integer :: id = 7
end type
type(pt), parameter :: origin = pt(0., 0.), unit_x = pt(1., 0.)
type(box), parameter :: unit_box = box(origin, pt(1., 2.), 3)
type(pt) :: last = pt(5., 6.)
contains
integer function counter()
  type(pt), save :: calls
  type(pt) :: fresh = pt(10., 0.)
  calls%x = calls%x + 1
  fresh%x = fresh%x + 1
  counter = nint(calls%x) * 100 + nint(fresh%x)
end function
end module
program p
use geo
implicit none
type(pt) :: q = pt(1., 2.)
type(box) :: b
integer :: i
print *, origin, unit_x
print *, unit_box
print *, q, last
b = unit_box
print *, b%hi%y, b%id
do i = 1, 3
  print *, counter()
end do
end program
""", ' 0.0 0.0 1.0 0.0\n 0.0 0.0 1.0 2.0 3\n 1.0 2.0 5.0 6.0\n 2.0 3\n 111\n 212\n 313\n')
        self.check("""module cfg
implicit none
private
type, public :: opts
  integer :: n = 10
  real :: tol = 1e-3
end type
type(opts), public, parameter :: defaults = opts(20, 1e-6)
type(opts), public :: current
type(opts), private :: hidden = opts(1, 2.)
public :: show
contains
subroutine show()
  print *, current, hidden%n
end subroutine
end module
program p
use cfg
implicit none
current = defaults
current%n = current%n + 1
call show()
print *, defaults%n * 2
end program
""", ' 21 1.e-006 1\n 40\n')

    def test_array_components_of_derived_type(self):
        """Components that are arrays of a derived type."""
        self.check("""module graphs
implicit none
type :: node
  integer :: id = 0
  real :: w = 1
end type
type :: graph
  character(len=8) :: label = 'g'
  type(node), allocatable :: nodes(:)
  type(node) :: root(2)
end type
contains
subroutine grow(g, n)
  type(graph), intent(inout) :: g
  integer, intent(in) :: n
  integer :: i
  if (allocated(g%nodes)) deallocate(g%nodes)
  allocate(g%nodes(n))
  do i = 1, n
    g%nodes(i)%id = 10*i
    g%nodes(i)%w = 0.5*i
  end do
end subroutine
real function total(g)
  type(graph), intent(in) :: g
  total = sum(g%nodes%w) + sum(g%root%w)
end function
end module
program p
use graphs
implicit none
type(graph) :: g, h
integer :: k
k = 3
call grow(g, k)
g%root(2) = node(5, 2.5)
print *, size(g%nodes), g%nodes(2)%id, g%nodes(3)%w, g%root(2)%id, g%root(1)%id
print *, total(g)
h = g
h%nodes(1)%id = -1
print *, g%nodes(1)%id, h%nodes(1)%id, size(h%nodes)
print *, g%nodes
print *, g%nodes(2:3)%id
end program
""", ' 3 20 1.5 5 0\n 6.5\n 10 -1 3\n 10 0.5 20 1.0 30 1.5\n 20 30\n')

    def test_arrays_of_records_with_array_and_nested_components(self):
        """Arrays of a derived type whose components are arrays or derived scalars."""
        self.check("""module particles
implicit none
type :: particle
  real :: pos(3) = 0
  integer :: id
  character(len=4) :: tag = 'none'
end type
contains
subroutine shift(ps, d)
  type(particle), intent(inout) :: ps(:)
  real, intent(in) :: d(3)
  integer :: i
  do i = 1, size(ps)
    ps(i)%pos = ps(i)%pos + d
  end do
end subroutine
real function spread_x(ps)
  type(particle), intent(in) :: ps(:)
  spread_x = maxval(ps%id) - minval(ps%id) + ps(size(ps))%pos(1)
end function
end module
program p
use particles
implicit none
type(particle) :: a(3), b(3)
type(particle), allocatable :: c(:)
integer :: i
do i = 1, 3
  a(i)%id = i
  a(i)%pos = [real(i), 2.0*i, 3.0*i]
end do
a(2)%pos(3) = -1
a(3)%tag(1:2) = 'ab'
print *, a(2)%pos, a(2)%pos(3), a(3)%id, sum(a(1)%pos), a(3)%tag
print *, a(1)
call shift(a, [10., 20., 30.])
print *, a(3)%pos(2:3), spread_x(a)
b = a
b(1) = particle([7., 8., 9.], 42, 'zz')
print *, b(1), b(2)%pos(1)
allocate(c(2))
c(2)%pos = 5
c(2)%id = 9
print *, c(2)%pos, c(1)%pos(1), c(2)%id, size(c)
print *, a
end program
""", ' 2.0 4.0 -1.0 -1.0 3 6.0 abne\n 1.0 2.0 3.0 1 none\n 26.0 39.0 15.0\n 7.0 8.0 9.0 42 zz   12.0\n 5.0 5.0 5.0 0.0 9 2\n 11.0 22.0 33.0 1 none 12.0 24.0 29.0 2 none 13.0 26.0 39.0 3 abne\n')
        self.check("""module shapes
implicit none
type :: point
  real :: x = 0, y = 0
end type
type :: circle
  type(point) :: center
  real :: r = 1
  integer :: tag(2) = [1, 2]
end type
contains
real function area_sum(cs)
  type(circle), intent(in) :: cs(:)
  area_sum = sum(3.0 * cs%r**2) + sum(cs%center%x)
end function
end module
program p
use shapes
implicit none
type(circle) :: c(3)
type(point) :: q
integer :: i
q = point(5., 6.)
do i = 1, 3
  c(i)%center%x = i
  c(i)%center%y = -i
  c(i)%r = 0.5 * i
end do
c(2) = circle(point(7., 8.), 2.5, [3, 4])
c(3) = circle(q, tag=[9, 9])
print *, c(2)%center%x, c(2)%center%y, c(3)%center%y, c(3)%r, c(2)%tag(2)
print *, c%center%x
print *, area_sum(c)
print *, c(1)
print *, c
end program
""", ' 7.0 8.0 6.0 1.0 4\n 1.0 7.0 5.0\n 35.5\n 1.0 -1.0 0.5 1 2\n 1.0 -1.0 0.5 1 2 7.0 8.0 2.5 3 4 5.0 6.0 1.0 9 9\n')

    def test_array_elements_as_objects(self):
        """An array element passed to a derived dummy, and type-bound calls on elements."""
        self.check("""module m
type :: pt
  real :: x = 0
contains
  procedure :: bump
end type
contains
subroutine bump(p, d)
  class(pt), intent(inout) :: p
  real, intent(in) :: d
  p%x = p%x + d
end subroutine
end module
program t
use m
type(pt) :: a(3)
integer :: i
do i = 1, 3
  call a(i)%bump(real(i))
end do
print *, a%x
end program
""", ' 1.0 2.0 3.0\n')
        self.check("""module m
type :: pt
  real :: x = 0
end type
contains
subroutine bump(p, d)
  type(pt), intent(inout) :: p
  real, intent(in) :: d
  p%x = p%x + d
end subroutine
end module
program t
use m
type(pt) :: a(3)
call bump(a(2), 5.)
print *, a%x
end program
""", ' 0.0 5.0 0.0\n')
        self.check("""module m
type :: pt
  real :: x = 0, y = 0
contains
  procedure :: norm
end type
contains
real function norm(p)
  class(pt), intent(in) :: p
  norm = sqrt(p%x**2 + p%y**2)
end function
real function dist(p, q)
  type(pt), intent(in) :: p, q
  dist = sqrt((p%x-q%x)**2 + (p%y-q%y)**2)
end function
end module
program t
use m
type(pt) :: a(3)
integer :: i
a(2) = pt(3., 4.)
a(3) = pt(6., 8.)
print *, dist(a(2), a(3)), a(3)%norm()
do i = 2, 3
  print *, dist(a(1), a(i))
end do
end program
""", ' 5.0 10.0\n 5.0\n 10.0\n')

    def test_keyword_constructors(self):
        """Structure constructors with keyword components."""
        self.check("""module m
type :: pt
real :: x = 0, y = 0
end type
end module
program p
use m
type(pt) :: a, b(2)
a = pt(y=2.)
b(2) = pt(y=3., x=1.)
print *, a, b(2)
end program
""", ' 0.0 2.0 1.0 3.0\n')

    def test_generic_bindings_and_defined_assignment(self):
        """GENERIC type-bound procedures, operators and assignment, and INTERFACE ASSIGNMENT(=)."""
        self.check("""module vec2
implicit none
private
type, public :: v2
  real :: x = 0, y = 0
contains
  procedure :: scale_r, scale_i
  procedure :: norm
  procedure, private :: plus_v, plus_s
  generic :: scale => scale_r, scale_i
  generic :: plus => plus_v, plus_s
end type
contains
subroutine scale_r(a, f)
  class(v2), intent(inout) :: a
  real, intent(in) :: f
  a%x = a%x*f; a%y = a%y*f
end subroutine
subroutine scale_i(a, f)
  class(v2), intent(inout) :: a
  integer, intent(in) :: f
  a%x = a%x + f; a%y = a%y + f
end subroutine
real function norm(a)
  class(v2), intent(in) :: a
  norm = sqrt(a%x**2 + a%y**2)
end function
function plus_v(a, b) result(c)
  class(v2), intent(in) :: a
  type(v2), intent(in) :: b
  type(v2) :: c
  c = v2(a%x + b%x, a%y + b%y)
end function
function plus_s(a, s) result(c)
  class(v2), intent(in) :: a
  real, intent(in) :: s
  type(v2) :: c
  c = v2(a%x + s, a%y + s)
end function
end module
program p
use vec2
implicit none
type(v2) :: a, b
a = v2(3., 4.)
print *, a%norm()
call a%scale(2.0)
call a%scale(1)
print *, a
b = a%plus(v2(1., 1.))
print *, b
b = a%plus(0.5)
print *, b, b%norm()
end program
""", ' 5.0\n 7.0 9.0\n 8.0 10.0\n 7.5 9.5 12.1037\n')
        self.check("""module fractions
implicit none
type :: frac
  integer :: n = 0, d = 1
contains
  procedure :: show
  procedure, private :: add_ff, from_real
  generic :: operator(+) => add_ff
  generic :: assignment(=) => from_real
end type
interface assignment(=)
  module procedure from_int
end interface
contains
subroutine from_int(f, i)
  type(frac), intent(out) :: f
  integer, intent(in) :: i
  f%n = i; f%d = 1
end subroutine
subroutine from_real(f, x)
  class(frac), intent(out) :: f
  real, intent(in) :: x
  f%n = nint(x * 100); f%d = 100
end subroutine
function add_ff(a, b) result(c)
  class(frac), intent(in) :: a
  type(frac), intent(in) :: b
  type(frac) :: c
  c%n = a%n*b%d + b%n*a%d
  c%d = a%d*b%d
end function
subroutine show(f)
  class(frac), intent(in) :: f
  print '(i0,"/",i0)', f%n, f%d
end subroutine
end module
program p
use fractions
implicit none
type(frac) :: a, b, c
a = 3
b = 0.25
c = a + b
call c%show()
c = frac(1, 2)
call c%show()
end program
""", '325/100\n1/2\n')

    def test_type_extension(self):
        """EXTENDS with inherited components and bindings, and overrides."""
        self.check("""module shapes
implicit none
type :: shape
  real :: x = 0, y = 0
  character(len=6) :: label = 'shape'
contains
  procedure :: move
  procedure :: describe
end type
type, extends(shape) :: circle
  real :: r = 1
contains
  procedure :: area => circle_area
end type
type, extends(circle) :: ring
  real :: inner = 0.5
end type
contains
subroutine move(s, dx, dy)
  class(shape), intent(inout) :: s
  real, intent(in) :: dx, dy
  s%x = s%x + dx
  s%y = s%y + dy
end subroutine
subroutine describe(s)
  class(shape), intent(in) :: s
  print '(a,2f6.2)', trim(s%label), s%x, s%y
end subroutine
real function circle_area(c)
  class(circle), intent(in) :: c
  circle_area = 3.0 * c%r**2
end function
real function ring_area(g)
  type(ring), intent(in) :: g
  ring_area = g%area() - 3.0 * g%inner**2
end function
end module
program p
use shapes
implicit none
type(circle) :: c
type(ring) :: g
type(shape) :: s
c%label = 'circle'
c%r = 2
call c%move(1., 2.)
call c%describe()
call describe(c)
print *, c%area(), c%shape%x, c%r
g = ring(1., 1., 'ring', 3., 1.)
call g%move(0.5, 0.5)
call g%describe()
print *, ring_area(g), g%circle%r, g%circle%shape%y
s = c%shape
call s%describe()
print *, g
end program
""", 'circle  1.00  2.00\ncircle  1.00  2.00\n 12.0 1.0 2.0\nring  1.50  1.50\n 24.0 3.0 1.5\ncircle  1.00  2.00\n 1.5 1.5 ring   3.0 1.0\n')

    def test_result_assigned_to_an_argument(self):
        """`r = f(r)` and `r = r * m` with array components read the old value of `r`."""
        self.check("""module mat
implicit none
type :: matrix
  real :: a(2,2) = 0
end type
interface operator(*)
  module procedure mul
end interface
contains
function mul(x, y) result(z)
  type(matrix), intent(in) :: x, y
  type(matrix) :: z
  z%a = matmul(x%a, y%a)
end function
function eye() result(m)
  type(matrix) :: m
  m%a = reshape([1., 0., 0., 1.], [2, 2])
end function
end module
program p
use mat
implicit none
type(matrix) :: m, r
m%a = reshape([1., 2., 3., 4.], [2, 2])
r = m * eye()
r = r * m
print *, r%a
end program
""", ' 7.0 10.0 15.0 22.0\n')
        self.check("""module m
type :: pt
  real :: x = 0, y = 0
end type
contains
function swap(p) result(q)
  type(pt), intent(in) :: p
  type(pt) :: q
  q%x = p%y
  q%y = p%x
end function
end module
program t
use m
type(pt) :: a
a = pt(1., 2.)
a = swap(a)
print *, a
end program
""", ' 2.0 1.0\n')

    def test_move_alloc_growth(self):
        """Growing an array with MOVE_ALLOC moves unset elements without an error."""
        self.check("""program p
implicit none
integer, allocatable :: items(:), tmp(:)
integer :: i, n
allocate(items(2))
n = 0
do i = 1, 5
  if (n == size(items)) then
    allocate(tmp(2*size(items)))
    tmp(1:n) = items(1:n)
    call move_alloc(tmp, items)
  end if
  n = n + 1
  items(n) = i
end do
print *, items(1:n)
end program
""", ' 1 2 3 4 5\n')
        self.check("""module st
implicit none
contains
subroutine push(items, n, v)
  integer, allocatable, intent(inout) :: items(:)
  integer, intent(inout) :: n
  integer, intent(in) :: v
  integer, allocatable :: tmp(:)
  if (.not. allocated(items)) allocate(items(4))
  if (n == size(items)) then
    allocate(tmp(2*size(items)))
    tmp(1:n) = items(1:n)
    call move_alloc(tmp, items)
  end if
  n = n + 1
  items(n) = v
end subroutine
end module
program p
use st
implicit none
integer, allocatable :: items(:)
integer :: i, n
n = 0
do i = 1, 10
  call push(items, n, i*i)
end do
print *, n, size(items), items(1:n)
end program
""", ' 10 16 1 4 9 16 25 36 49 64 81 100\n')
        self.check("""module stack_mod
implicit none
type :: stack
  integer, allocatable :: items(:)
  integer :: n = 0
contains
  procedure :: push, pop, size => stack_size
end type
contains
subroutine push(s, v)
  class(stack), intent(inout) :: s
  integer, intent(in) :: v
  integer, allocatable :: tmp(:)
  if (.not. allocated(s%items)) allocate(s%items(4))
  if (s%n == size(s%items)) then
    allocate(tmp(2*size(s%items)))
    tmp(1:s%n) = s%items(1:s%n)
    call move_alloc(tmp, s%items)
  end if
  s%n = s%n + 1
  s%items(s%n) = v
end subroutine
integer function pop(s)
  class(stack), intent(inout) :: s
  pop = s%items(s%n)
  s%n = s%n - 1
end function
integer function stack_size(s)
  class(stack), intent(in) :: s
  stack_size = s%n
end function
end module
program p
use stack_mod
implicit none
type(stack) :: s
integer :: i
do i = 1, 10
  call s%push(i*i)
end do
print *, s%size(), size(s%items)
print *, s%pop(), s%pop(), s%size()
end program
""", ' 10 16\n 100 81 8\n')

    def test_interpreter_converts_stored_values(self):
        """Storing an INTEGER value into a REAL array keeps the array REAL."""
        self.check("""program p
real :: x(3)
x = 0.5
x(3) = -1
x(1) = 7.5
print *, x
end program
""", ' 7.5 0.5 -1.0\n')
        self.check("""program p
real, allocatable :: a(:)
allocate(a(2))
a = 1
a(2) = 0.5
print *, a
end program
""", ' 1.0 0.5\n')
        self.check("""program p
integer, allocatable :: a(:)
allocate(a(2))
a = 2.7
print *, a
end program
""", ' 2 2\n')

    def test_dynamic_dispatch_is_diagnosed(self):
        """A binding called through a CLASS dummy that an extension overrides
        needs dynamic dispatch, which is rejected rather than misdirected."""
        self.reject("""module zoo
implicit none
type :: animal
  integer :: legs = 4
contains
  procedure :: speak
end type
type, extends(animal) :: bird
contains
  procedure :: speak => chirp
end type
contains
subroutine speak(a)
  class(animal), intent(in) :: a
  print *, 'generic', a%legs
end subroutine
subroutine chirp(a)
  class(bird), intent(in) :: a
  print *, 'chirp', a%legs
end subroutine
subroutine talk(a)
  class(animal), intent(in) :: a
  call a%speak()
end subroutine
end module
program p
use zoo
type(bird) :: b
b%legs = 2
call b%speak()
call talk(b)
end program
""", "needs dynamic dispatch")


if __name__ == "__main__":
    unittest.main(verbosity=2)
