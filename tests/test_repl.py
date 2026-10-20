"""Persistent scalar and fixed-array REPL, including runtime-error recovery.

Run after cargo build --bin rfort: python tests/test_repl.py
RFORT_TEST_COMPILER can select the release executable.
"""
import subprocess
import unittest

from test_record_read import COMPILER


class ScalarRepl(unittest.TestCase):
    def session(self, source, expected, errors=()):
        result = subprocess.run([COMPILER, "repl"], input=source,
                                text=True, capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        for message in errors:
            self.assertIn(message, result.stderr)
        if not errors:
            self.assertEqual(result.stderr, "")
        return result

    def test_scalars_expressions_and_precision(self):
        self.session("""integer :: n
n=10
print *, n**2
n=n+1
n
real :: x=9
sqrt(x)
double precision :: d=1.23456789012345d0
d
d=d+1d0
d
logical :: yes=.true.
.not.yes
character(5) :: s='hi'
s
s='world'
s
:quit
""", " 100\n 11\n 3.0\n 1.23456789012345\n 2.23456789012345\n F\n hi   \n world\n")

    def test_no_replay_and_parameter_constants(self):
        self.session("""integer, parameter :: k=3
integer :: n=k*2
print *, 'once'
n=n+1
integer :: z=4
n
k+z
:quit
""", " once\n 7\n 7\n")

    def test_multiline_blocks_and_continuation(self):
        self.session("""integer :: i,n
n=0
do i=1,3
  if (i>1) then
    n=n+i
  end if
end do
n
select case (n)
case (5)
print *, 'five'
case default
print *, 'wrong'
end select
n = n + & ! a continuation
! intervening comment
 & 2
n
print *, 'hello!'
:quit
""", " 5\n five\n 7\n hello!\n")

    def test_random_state_is_not_restarted_or_replayed(self):
        self.session("""real :: x,y
call random_number(x)
call random_number(y)
print *, x,y
:reset
real :: x,y
call random_number(x)
call random_number(y)
print *, x,y
:quit
""", " 0.280835 0.671137\n 0.280835 0.671137\n")

    def test_initializers_are_evaluated_once(self):
        self.session("""integer :: n=1
n=n+1
n
n=n+1
n
:quit
""", " 2\n 3\n")

    def test_formatted_output_and_statements_on_one_line(self):
        self.session("""integer :: n=3
n=n+1; print '(i3)',n
write(*,'(a,i2)') 'n=',n
write(*,*) n+1
:quit
""", "  4\nn= 4\n 5\n")

    def test_initialization_on_taken_branch_persists(self):
        self.session("""integer :: n
logical :: condition=.true.
if(condition) then
n=8
end if
n
:quit
""", " 8\n")

    def test_compile_errors_do_not_corrupt_session(self):
        self.session("""integer :: n=5
integer :: n
integer :: =
unknown+1
integer, parameter :: k=3
k=9
integer, pointer :: a(:)
n
k
:quit
""", " 5\n 3\n", ("already declared", "parse failed", "undefined variable",
                           "cannot modify PARAMETER", "unsupported in the REPL"))

    def test_uninitialized_values_are_not_saved_as_zero(self):
        self.session("""integer :: n
n
logical :: condition=.false.
if(condition) then
n=8
end if
n
n=4
n
:quit
""", " 4\n", ("uninitialized variable `n`",))

    def test_runtime_error_rolls_back_scalar_changes(self):
        result = self.session("""integer :: n=5,z=0
if(.true.) then
n=n+1
n=n/z
end if
n
n=n+1
n
:quit
""", " 5\n 6\n", ("integer division by zero", "previous scalar and array values retained"))
        self.assertNotIn("panicked", result.stderr)

    def test_cancel_reset_and_eof(self):
        self.session("""integer :: n=7
if(.true.) then
n=9
:cancel
n
:reset
real :: n=2
n
""", " 7\n 2.0\n")
        self.session("integer :: n=3\ndo n=1,2\n", "", ("incomplete input",))

    def test_help_unknown_command_and_quit(self):
        result = subprocess.run([COMPILER, "repl"], input=":help\n:oops\n:q\nprint *, 99\n",
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(":reset", result.stdout)
        self.assertIn("unknown REPL command", result.stderr)
        self.assertNotIn("99", result.stdout)

    def test_integer_array_state_sections_and_expressions(self):
        self.session("""integer, parameter :: n=3
integer :: a(n)
a=[1,2,3]
a(2)=10
sum(a)
a(1:3:2)=7
a
a+1
:quit
""", " 14\n 7 10 7\n 8 11 8\n")

    def test_array_initializers_only_execute_once(self):
        self.session("""integer :: a(3)=[1,2,3]
a(2)=9
a
integer :: n=5
a
:quit
""", " 1 9 3\n 1 9 3\n")

    def test_real_and_logical_arrays(self):
        self.session("""real :: x(2)
double precision :: d(2)
logical :: mask(2)
x=[1.5,2.5]
d=[1.23456789012345d0,2d0]
mask=x>2
mask
sum(x)
abs(d(1)-1.23456789012345d0)<1d-14
:quit
""", " F T\n 4.0\n T\n")

    def test_multidimensional_arrays_and_nonunit_bounds(self):
        self.session("""integer :: a(-1:0,2:3)
a=0
a(-1,3)=7
a(0,2)=4
a
a(-1,3)+a(0,2)
:quit
""", " 0 4 7 0\n 11\n")

    def test_partial_initialization_and_recovery(self):
        self.session("""integer :: a(3)
a(2)=8
a(2)
a(1)
a(2)
a(1)=4
a(1:2)
:quit
""", " 8\n 8\n 4 8\n", ("uninitialized variable `a`",))

    def test_runtime_failure_rolls_back_array_and_scalar_state(self):
        self.session("""integer :: a(2)=[1,2], n=3, z=0
if (.true.) then
a(1)=9
n=10
n=n/z
end if
a
n
:quit
""", " 1 2\n 3\n", ("integer division by zero", "previous scalar and array values retained"))

    def test_runtime_uninitialized_array_read_retains_state(self):
        self.session("""integer :: a(2)
integer :: i=1
a(2)=8
a(i)
a(2)
:quit
""", " 8\n", ("uninitialized variable `a`", "previous scalar and array values retained"))

    def test_array_parameters_and_empty_arrays(self):
        self.session("""integer, parameter :: p(2)=[2,3]
integer :: empty(0)
sum(empty)
sum(p)
p(1)=9
sum(p)
:quit
""", " 0\n 5\n 5\n", ("cannot modify PARAMETER",))

    def test_random_array_state_and_reset(self):
        self.session("""real :: a(2)
call random_number(a)
a
real :: x
call random_number(x)
x
a
:reset
real :: a(2)
call random_number(a)
a
:quit
""", " 0.280835 0.671137\n 0.725846\n 0.280835 0.671137\n 0.280835 0.671137\n")

    def test_array_limits_and_reserved_helpers(self):
        self.session("""integer :: a(10001)
integer :: rfort_repl_snapshot_0
character(3) :: s(2)
integer :: n=7
n
:quit
""", " 7\n", ("snapshot limit", "reserved by the REPL", "fixed-size INTEGER, REAL and LOGICAL"))

    def test_large_sparse_array(self):
        # The snapshot adapter must not create scalar runtime storage for
        # every unset element. Previously this declaration alone took over a
        # minute for 10000 elements. Exercise subsequent sparse restores too.
        result = subprocess.run([COMPILER, "repl"], input="""real :: x(10000)
x(10000)=3.5
x(10000)
x(1)
x(10000)
:quit
""", text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, " 3.5\n 3.5\n")
        self.assertIn("uninitialized variable `x`", result.stderr)

    def test_large_initialized_array(self):
        # Fully initialized snapshots must not add one scalar variable per
        # element, which made each following submission quadratic in size.
        result = subprocess.run([COMPILER, "repl"], input="""real :: x(10000)
call random_number(x)
x(1)
x(10000)=0.5
x(10000)
x(1)
:quit
""", text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertEqual(result.stdout, " 0.280835\n 0.5\n 0.280835\n")

    def test_packed_snapshots_preserve_numeric_ranges_and_special_values(self):
        self.session("""double precision :: x(4)=[1d-300,1d300,-0d0,1.23456789012345d0]
abs(x(1)/1d-300-1d0)<1d-14
abs(x(2)/1d300-1d0)<1d-14
1d0/x(3)<0d0
abs(x(4)-1.23456789012345d0)<1d-14
integer(kind=8) :: n(2)=[-9223372036854775807_8,9223372036854775807_8]
n
x(1)=0d0/0d0
x(1)/=x(1)
x(2)=1d0/0d0
x(2)>1d300
:quit
""", " T\n T\n T\n T\n -9223372036854775807 9223372036854775807\n T\n T\n")

    def test_auto_array_declarations_and_self_resizing(self):
        self.session(""":auto on
x=[10.0,20.0,30.0]
x
x=[x,40.0]
x
x=x(2:3)
x
x=2.5
x
:auto off
x=[7.0]
x
:info
:quit
""", " 10.0 20.0 30.0\n 10.0 20.0 30.0 40.0\n 20.0 30.0\n 2.5 2.5\n 7.0\nreal(4), allocatable :: x(:) ! allocated, size=1\n")

    def test_auto_scalar_types_constants_and_array_kinds(self):
        self.session(""":auto on
n=3
integer, parameter :: k=2
integer :: i
a=[(i,i=1,k)]
a
d=[1d0,2d0]
kind(d)
total=sum(d)
total
root=sqrt(9.)
root
mask=d>1d0
mask
s='hello'
s
n=n+1
n
:quit
""", " 1 2\n 8\n 3.0\n 3.0\n F T\n hello\n 4\n")

    def test_explicit_allocatable_arrays(self):
        self.session("""real, allocatable :: x(:)
allocated(x)
x=[1.,2.]
allocated(x)
size(x)
deallocate(x)
allocated(x)
integer, parameter :: n=3
allocate(x(n))
x(2)=8.
x(2)
x(1)
x=4.
x
deallocate(x)
allocate(x(0))
allocated(x)
size(x)
:quit
""", " F\n T\n 2\n F\n 8.0\n 4.0 4.0 4.0\n T\n 0\n", ("uninitialized variable",))

    def test_auto_mode_and_allocation_roll_back_on_failure(self):
        self.session(""":auto on
x=[1,2]
integer :: z=0,n=7
x=[3,4,5]; n=n/z
x
size(x)
n
y=[3,4]; n=n/z
:info
:quit
""", " 1 2\n 2\n 7\ninteger(4) :: n\ninteger(4), allocatable :: x(:) ! allocated, size=2\ninteger(4) :: z\n",
                     ("integer division by zero", "previous scalar and array values retained"))

    def test_auto_strict_mode_rank_and_existing_types(self):
        self.session("""x=[1.,2.]
:auto on
x=[1.,2.]
x=[1,2,3]
kind(x)
x
x=[.true.]
x
x=reshape([1.,2.,3.,4.],[2,2])
x
:auto off
y=3
:auto
:reset
:auto
:info
:quit
""", " 4\n 1.0 2.0 3.0\n 1.0 2.0 3.0\n 1.0 2.0 3.0\nautomatic declarations: off\nautomatic declarations: off\n",
                     ("undefined variable", "rank one"))

    def test_allocation_errors_and_multiple_assignments(self):
        self.session("""integer, allocatable :: a(:)
a
a=2
deallocate(a)
allocate(a(2))
allocate(a(3))
a(1)=7
a(1)
:auto on
b=[1,2]; b=[b,3]; b=[b,4]
b
:quit
""", " 7\n 1 2 3 4\n", ("not allocated", "already allocated"))

    def test_empty_allocatable_and_type_inquiries(self):
        self.session("""real, allocatable :: x(:)
kind(x)
if (allocated(x)) then
print *, x
end if
:auto on
x=[real ::]
allocated(array=x)
size(x)
x=[1.,2.]
size(x)
:quit
""", " 4\n T\n 0\n 2\n")


if __name__ == "__main__":
    unittest.main()
