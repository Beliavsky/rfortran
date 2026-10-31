"""Recursive derived types, whose POINTER or ALLOCATABLE components are of
their own type: linked lists with POINTER components, ALLOCATABLE `next`
and child components, binary trees built and walked by recursive
procedures, deep copies and DEALLOCATE of whole subtrees, stacks in modules
with CHARACTER and array components, structure constructors, TARGET
objects, type-bound procedures, ALLOCATE with SOURCE=, MOVE_ALLOC and
functions returning pointers. Also recursive subroutines with scalar dummy
arguments they define.

Each case is checked in the interpreter, at -O0 and -O2, and against
gfortran where gfortran runs it.
Run after cargo build --bin rfort: python tests/test_recursive_types.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class RecursiveTypes(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_pointer_linked_list(self):
        self.check("""program p
  type node
    integer :: v
    type(node), pointer :: next => null()
  end type
  type(node), pointer :: head, cur
  integer i, total
  head => null()
  do i = 1, 4
    allocate(cur)
    cur%v = i * i
    cur%next => head
    head => cur
  end do
  total = 0
  cur => head
  do while (associated(cur))
    total = total + cur%v
    write (*, '(i0,1x)', advance='no') cur%v
    cur => cur%next
  end do
  print *
  print *, total, associated(head%next%next%next), associated(head%next%next%next%next)
end program
""", "16 9 4 1\n30 T F\n")

    def test_allocatable_components_and_copies(self):
        self.check("""program p
  type node
    integer :: v
    type(node), allocatable :: next
  end type
  type(node) :: head, other
  head%v = 1
  allocate(head%next)
  head%next%v = 2
  allocate(head%next%next)
  head%next%next%v = 3
  other = head
  other%next%v = 20
  print *, head%next%v, other%next%v, other%next%next%v
  deallocate(head%next)
  print *, allocated(head%next), allocated(other%next%next)
  call move_alloc(other%next, head%next)
  print *, allocated(other%next), head%next%v, head%next%next%v
end program
""", "2 20 3\nF T\nF 20 3\n")

    def test_binary_tree(self):
        # gfortran 17 fails with a segmentation fault on this program.
        self.check("""module trees
  implicit none
  type node
    integer :: key
    type(node), allocatable :: left, right
  end type
contains
  recursive subroutine insert(t, k)
    type(node), allocatable, intent(inout) :: t
    integer, intent(in) :: k
    if (.not. allocated(t)) then
      allocate(t)
      t%key = k
    else if (k < t%key) then
      call insert(t%left, k)
    else
      call insert(t%right, k)
    end if
  end subroutine
  recursive subroutine walk(t)
    type(node), intent(in) :: t
    if (allocated(t%left)) call walk(t%left)
    write (*, '(i0,1x)', advance='no') t%key
    if (allocated(t%right)) call walk(t%right)
  end subroutine
  recursive integer function depth(t) result(d)
    type(node), intent(in) :: t
    integer :: l, r
    l = 0
    r = 0
    if (allocated(t%left)) l = depth(t%left)
    if (allocated(t%right)) r = depth(t%right)
    d = 1 + max(l, r)
  end function
end module
program p
  use trees
  implicit none
  type(node), allocatable :: root, copy
  integer :: keys(9) = [50, 30, 70, 20, 40, 60, 80, 35, 65]
  integer :: i
  do i = 1, size(keys)
    call insert(root, keys(i))
  end do
  call walk(root)
  print *
  print *, depth(root)
  copy = root
  call insert(copy, 10)
  deallocate(root%left)
  call walk(root)
  print *
  call walk(copy)
  print *
  print *, depth(copy), allocated(root%left)
end program
""", "20 30 35 40 50 60 65 70 80\n4\n50 60 65 70 80\n10 20 30 35 40 50 60 65 70 80\n4 F\n", reference=False)

    def test_stack_module(self):
        self.check("""module stacks
  implicit none
  type item
    character(len=8) :: name
    real :: values(3) = 0
    type(item), pointer :: below => null()
  end type
  type stack
    type(item), pointer :: top => null()
    integer :: count = 0
  end type
contains
  subroutine push(s, name, x)
    type(stack), intent(inout) :: s
    character(len=*), intent(in) :: name
    real, intent(in) :: x
    type(item), pointer :: new
    allocate(new)
    new%name = name
    new%values = [x, 2*x, 3*x]
    new%below => s%top
    s%top => new
    s%count = s%count + 1
  end subroutine
  subroutine pop(s, it)
    type(stack), intent(inout) :: s
    type(item), pointer, intent(out) :: it
    it => s%top
    if (associated(it)) then
      s%top => it%below
      s%count = s%count - 1
    end if
  end subroutine
end module
program p
  use stacks
  implicit none
  type(stack) :: s
  type(item), pointer :: it
  type(item), target :: fixed
  call push(s, 'one', 1.0)
  call push(s, 'two', 2.0)
  call push(s, 'three', 3.0)
  print *, s%count, s%top%name, s%top%below%name
  do
    call pop(s, it)
    if (.not. associated(it)) exit
    print *, trim(it%name), sum(it%values), it%values(2)
    deallocate(it)
  end do
  fixed = item('fixed', [1., 2., 3.], null())
  it => fixed
  it%values(1) = 9
  print *, fixed%values, associated(it, fixed), s%count
end program
""", "3 three two\nthree 18.0 6.0\ntwo 12.0 4.0\none 6.0 2.0\n9.0 2.0 3.0 T 0\n")

    def test_type_bound_and_source(self):
        self.check("""module lists
  type node
    integer :: v
    type(node), pointer :: next => null()
  contains
    procedure :: show => show_node
    procedure :: length
  end type
contains
  subroutine show_node(self)
    class(node), intent(in) :: self
    print *, 'node', self%v
  end subroutine
  recursive integer function length(self) result(n)
    class(node), intent(in) :: self
    n = 1
    if (associated(self%next)) n = 1 + length(self%next)
  end function
  function last(p) result(q)
    type(node), pointer, intent(in) :: p
    type(node), pointer :: q
    q => p
    do while (associated(q%next))
      q => q%next
    end do
  end function
end module
program p
  use lists
  type(node), pointer :: a, b
  allocate(a, source=node(1, null()))
  allocate(a%next, source=node(2, null()))
  allocate(a%next%next)
  a%next%next%v = 3
  call a%show()
  call a%next%show()
  b => last(a)
  print *, a%length(), b%v, associated(b, a%next%next)
end program
""", "node 1\nnode 2\n3 3 T\n")

    def test_recursive_output_arguments(self):
        self.check("""module m
contains
  recursive subroutine count(n, acc)
    integer, intent(in) :: n
    integer, intent(inout) :: acc
    if (n <= 0) return
    acc = acc + n
    call count(n - 1, acc)
  end subroutine
  recursive subroutine even(n, r)
    integer, intent(in) :: n
    logical, intent(out) :: r
    if (n == 0) then
      r = .true.
    else
      call odd(n - 1, r)
    end if
  end subroutine
  recursive subroutine odd(n, r)
    integer, intent(in) :: n
    logical, intent(out) :: r
    if (n == 0) then
      r = .false.
    else
      call even(n - 1, r)
    end if
  end subroutine
end module
program p
  use m
  integer :: s, v(2)
  logical :: e
  s = 0
  call count(4, s)
  v = 0
  call count(3, v(2))
  call even(7, e)
  print *, s, v, e
end program
""", "10 0 6 F\n")

    def test_rejections(self):
        self.reject("""program p
  type node
    integer :: v
    type(node), pointer :: next => null()
  end type
  type, extends(node) :: named
    character(len=4) :: name
  end type
  type(named) :: x
end program
""", "extension of recursive type `node` is not yet supported")


if __name__ == "__main__":
    unittest.main()
