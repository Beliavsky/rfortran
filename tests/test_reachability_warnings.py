"""Conservative unreachable code diagnostics and private-module liveness."""
import unittest
from pathlib import Path
import subprocess
import tempfile
import test_unused_warnings as shared
from test_record_read import ROOT, COMPILER


class ReachabilityWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_example_and_options(self):
        source = (ROOT/'tests/programs/xdead.f90').read_text(encoding='utf-8')
        self.assertEqual(self.check(source), '')
        for flags in [['--wall'], ['-Wunreachable-code']]:
            warning = self.check(source, flags)
            self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
            self.assertIn('example.f90:6:1', warning)
        self.assertEqual(self.check(source, ['--wall', '-Wno-unreachable-code']), '')
        self.assertEqual(self.check(source, ['-Wunreachable-code', '-Wno-unreachable-code']), '')
        self.assertIn('[unreachable-code]', self.check(source, ['-Wno-unreachable-code', '-Wunreachable-code']))
        self.assertIn('error: unreachable', self.check(source, ['-Werror=unreachable-code'], errors=True))
        self.assertEqual(self.check('!#allow(unreachable-code)\n'+source, ['--wall']), '')
        self.assertIn('[unreachable-code]', self.check('!#deny(unreachable-code)\n'+source, errors=True))

    def test_terminators_warn_once_per_sequence(self):
        for terminator in ['stop', 'stop 1', 'error stop', 'error stop "failed"', 'return']:
            with self.subTest(terminator=terminator):
                source = f'''program main
call work()
contains
subroutine work()
{terminator}
print *,1
print *,2
end subroutine
end program
'''
                warning = self.check(source, ['-Wunreachable-code'])
                self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
                self.assertIn('example.f90:6:1', warning)

    def test_conditional_and_both_branches_terminate(self):
        source = '''program main
call work(.true.)
contains
subroutine work(condition)
logical :: condition
if (condition) then
return
else
stop
end if
print *,2
end subroutine
end program
'''
        self.assertEqual(self.check(source, ['-Wunreachable-code']).count('[unreachable-code]'), 1)
        source = source.replace('else\nstop\n', '')
        self.assertEqual(self.check(source, ['-Wunreachable-code']), '')

    def test_constant_parameters_and_expressions(self):
        for condition, declarations in [
            ('.false.', ''), ('2>3', ''),
            ('debug', 'logical,parameter :: debug=.false.'),
            ('n>3', 'integer,parameter :: n=2'),
            ('x>3.0', 'real,parameter :: x=2.0'),
            ('text=="yes"', 'character(*),parameter :: text="no"'),
            ('.not. active', 'logical,parameter :: active=.true.'),
            ('debug', 'logical,parameter :: active=.true., debug=.not.active'),
        ]:
            with self.subTest(condition=condition):
                warning = self.check(f'''{declarations}
if ({condition}) then
print *,1
print *,2
end if
print *,3
end
''', ['--wall', '-Wno-unused'])
                self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
                self.assertIn('unreachable THEN branch', warning)
                self.assertIn('example.f90:3:1', warning)
        warning = self.check('if (.true.) then\nprint *,1\nelse\nprint *,2\nend if\nend\n', ['--wall'])
        self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
        self.assertIn('unreachable ELSE branch', warning)

    def test_variable_values_are_not_assumed_constant(self):
        self.assertEqual(self.check('''logical :: condition=.false.
if (condition) print *,1
condition=.true.
if (condition) print *,2
end
''', ['-Wunreachable-code']), '')

    def test_single_line_constant_if(self):
        warning = self.check('if (.false.) print *,1\nend\n', ['-Wunreachable-code'])
        self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
        self.assertIn('example.f90:1:', warning)

    def test_user_procedure_shadowing_intrinsic(self):
        self.assertEqual(self.check('''program main
if (abs(-1)==2) print *,1
contains
integer function abs(n)
integer :: n
abs=2
end function
end program
''', ['-Wunreachable-code']), '')

    def test_empty_branches_and_synthetic_result_return(self):
        self.assertEqual(self.check('''program main
print *, f()
contains
integer function f()
f=1
stop
end function
end program
''', ['-Wunreachable-code']), '')
        self.assertEqual(self.check('if (.false.) then\nend if\nend\n', ['-Wunreachable-code']), '')

    def test_loops_do_not_make_following_statements_dead(self):
        warning = self.check('''integer :: i
do i=1,2
cycle
print *,i
end do
print *,3
do while (.false.)
print *,4
end do
print *,5
end
''', ['-Wunreachable-code'])
        self.assertEqual(warning.count('[unreachable-code]'), 2, warning)
        self.assertIn('example.f90:4:1', warning)
        self.assertIn('example.f90:8:1', warning)

    def test_private_entities_only_referenced_in_dead_code(self):
        warning = self.check('''module m
implicit none
private
public :: entry
integer :: hidden_variable=1
integer,parameter :: hidden_constant=2
contains
subroutine entry()
return
print *,hidden_variable,hidden_constant
call hidden_procedure()
end subroutine
subroutine hidden_procedure()
end subroutine
end module
program main
use m
call entry()
end program
''', ['-Wunused-private'])
        self.assertEqual(warning.count('warning:'), 3, warning)
        for name in ['hidden_variable', 'hidden_constant', 'hidden_procedure']:
            self.assertIn('`'+name+'`', warning)

    def test_module_constants_and_host_shadowing(self):
        source = '''module m
logical,parameter :: debug=.false.
end module
program main
use m, only: enabled=>debug
if (enabled) then
print *,1
end if
call work(.true.)
contains
subroutine work(enabled)
logical :: enabled
if (enabled) print *,2
end subroutine
end program
'''
        warning = self.check(source, ['-Wunreachable-code'])
        self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
        self.assertIn('example.f90:7:1', warning)

    def test_labels_allow_another_entry(self):
        warning = self.check('''goto 10
stop
10 print *,1
print *,2
end
''', ['-Wunreachable-code'])
        self.assertEqual(warning.count('[unreachable-code]'), 1, warning)
        self.assertIn('example.f90:2:1', warning)

    def test_separate_module_constant_and_private_liveness(self):
        with tempfile.TemporaryDirectory(prefix='reachability-module-') as tmp:
            root = Path(tmp)
            cfg = root/'cfg.f90'
            cfg.write_text('module cfg\nlogical,parameter :: debug=.false.\nend module\n', encoding='utf-8')
            result = subprocess.run([COMPILER, '-c', cfg], cwd=root, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            worker = root/'worker.f90'
            worker.write_text('''module worker
use cfg
private
public :: entry
contains
subroutine entry()
if (debug) call hidden()
end subroutine
subroutine hidden()
end subroutine
end module
''', encoding='utf-8')
            result = subprocess.run([COMPILER, '--wall', 'check', worker], cwd=root,
                                    capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertEqual(result.stderr.count('[unreachable-code]'), 1, result.stderr)
            self.assertIn('private module procedure `hidden`', result.stderr)
            self.assertIn('worker.f90:7:', result.stderr)
            self.assertNotIn('cfg.f90:', result.stderr)

    def test_nested_else_if_and_select_termination(self):
        warning = self.check('''integer :: i
i=1
if (.true.) then
print *,1
else if (i==2) then
print *,2
end if
select case(i)
case(1)
stop
case default
error stop
end select
print *,3
end
''', ['-Wunreachable-code'])
        self.assertEqual(warning.count('[unreachable-code]'), 2, warning)
        self.assertIn('example.f90:5:1', warning)
        self.assertIn('example.f90:14:1', warning)


if __name__ == '__main__':
    unittest.main()
