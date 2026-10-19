"""Persistent scalar REPL, including worker runtime-error recovery.

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
integer :: a(2)
n
k
:quit
""", " 5\n 3\n", ("already declared", "parse failed", "undefined variable",
                           "cannot modify PARAMETER", "unsupported in the scalar REPL"))

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
""", " 5\n 6\n", ("integer division by zero", "previous scalar values retained"))
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


if __name__ == "__main__":
    unittest.main()
