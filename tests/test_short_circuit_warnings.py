"""Allocation/association guards cannot rely on logical short-circuiting."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_unused_warnings as shared
from test_record_read import COMPILER
from test_record_read import ROOT


def array_program(body, before='', extra=''):
    return f'''program main
real,allocatable::x(:)
allocate(x(2))
x=1.0
call work(x)
contains
subroutine work(x)
real,allocatable,intent(in)::x(:)
{before}
{body}
end subroutine
{extra}
end program
'''


class ShortCircuitWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_controls_and_value_location(self):
        source = array_program('if(allocated(x).and.size(x)>0) print *,x(1)')
        self.assertEqual(self.check(source), '')
        for flag in ('-Wshort-circuit', '--wall'):
            warning = self.check(source, [flag])
            self.assertEqual(warning.count('[short-circuit]'), 1, warning)
            self.assertIn('as ALLOCATED', warning)
            self.assertIn('Fortran .AND. and .OR.', warning)
            # The highlight should select SIZE's argument, not ALLOCATED's.
            self.assertIn('example.f90:10:26', warning)
        self.assertEqual(self.check(source, ['--wall', '-Wno-short-circuit']), '')
        self.assertEqual(self.check(source, ['-Wshort-circuit', '-Wno-short-circuit']), '')
        self.assertIn('[short-circuit]', self.check(source, ['-Wno-short-circuit', '-Wshort-circuit']))
        for flags in (['-Werror=short-circuit'], ['-Wshort-circuit', '-Werror']):
            self.assertIn('error: access', self.check(source, flags, errors=True))
        self.assertEqual(self.check('!#allow(short-circuit)\n'+source, ['--wall']), '')
        self.assertIn('[short-circuit]', self.check('!#deny(short-circuit)\n'+source, errors=True))
        self.assertEqual(self.check(source, ['-Werror']), '')

    def test_example_and_unchanged_runtime_behavior(self):
        source = (ROOT/'tests/programs/xshort_circuit_warnings.f90').read_text()
        warning = self.check(source, ['-Wall'])
        self.assertEqual(warning.count('[short-circuit]'), 2, warning)
        import test_deallocate
        test_deallocate.Deallocate.check(self, source, '2\n1\n')

    def test_or_reversed_and_multiple_accesses(self):
        for expr in ('size(x)>0.and.allocated(x)',
                     '.not.allocated(x).or.size(x)==0',
                     'allocated(array=x).and.sum(x)>0',
                     'allocated(x).and.size(x)>0.and.x(1)>0'):
            with self.subTest(expr=expr):
                warning = self.check(array_program(f'if({expr}) print *,1'), ['-Werror=short-circuit'], errors=True)
                self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_nested_guards_and_early_returns(self):
        for body in ('''if(allocated(x))then
if(allocated(x).and.size(x)>0)print *,1
end if''',
                     '''if(.not.allocated(x))return
if(allocated(x).and.size(x)>0)print *,1''',
                     '''if(.not.allocated(x))then
print *,0
else
if(allocated(x).and.sum(x)>0)print *,1
end if'''):
            with self.subTest(body=body):
                self.assertEqual(self.check(array_program(body), ['-Wshort-circuit']), '')

    def test_prior_guard_does_not_escape_branch(self):
        warning = self.check(array_program('''if(allocated(x))print *,1
if(allocated(x).and.size(x)>0)print *,1'''), ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_safe_inquiries(self):
        self.assertEqual(self.check(array_program('''if(allocated(x).and.allocated(x))print *,1
if(allocated(x).and.kind(x)==kind(1.0))print *,1'''), ['-Wshort-circuit']), '')
        self.assertEqual(self.check(array_program('if(allocated(x).and.kind(x+1.0)==kind(1.0))print *,1'),
                                    ['-Wshort-circuit']), '')
        self.assertEqual(self.check('''subroutine work(x)
character(4),allocatable,intent(in)::x(:)
if(allocated(x).and.len(x)==4)print *,1
end subroutine
''', ['-Wshort-circuit']), '')

    def test_non_deferred_character_length(self):
        self.assertEqual(self.check('''subroutine work(x,n)
integer,intent(in)::n
character(n),allocatable,intent(in)::x(:)
if(allocated(x).and.len(x)>0)print *,1
end subroutine
''', ['-Wshort-circuit']), '')

    def test_lazy_conditional_expressions(self):
        self.assertEqual(self.check(array_program('''if((allocated(x) ? size(x)>0 : .false.))print *,1
if(allocated(x).and.(allocated(x) ? sum(x)>0 : .false.))print *,1'''), ['-Wshort-circuit']), '')

    def test_known_descriptor_and_optional_dummies(self):
        for declaration in ('real,allocatable,intent(in)::a(:)', 'real,optional,intent(in)::a(:)'):
            extra = f'''logical function inspect(a)
{declaration}
inspect=.true.
end function'''
            warning = self.check(array_program('if(allocated(x).and.inspect(x))print *,1', extra=extra),
                                 ['-Wshort-circuit'])
            self.assertNotIn('[short-circuit]', warning)

    def test_safe_allocation_before_expression(self):
        self.assertEqual(self.check('''real,allocatable::x(:)
allocate(x(2))
x=1
print *,x
if(allocated(x).and.size(x)>0)print *,x
end
''', ['-Wshort-circuit']), '')

    def test_scalar_and_array_pointers(self):
        for declaration, access in (('real,pointer,intent(in)::p', 'p>0'),
                                    ('real,pointer,intent(in)::p(:)', 'size(p)>0')):
            source = f'''subroutine work(p)
{declaration}
if(associated(p).and.{access})print *,1
end subroutine
'''
            warning = self.check(source, ['-Werror=short-circuit'], errors=True)
            self.assertEqual(warning.count('[short-circuit]'), 1, warning)
            self.assertIn('as ASSOCIATED', warning)

    def test_pointer_guard_and_two_argument_inquiry(self):
        self.assertEqual(self.check('''real,target::x=1.0
real,pointer::p
p=>x
if(associated(p).and.associated(p,x)) print *,1
if(.not.associated(p))stop
if(associated(p).and.p>0)print *,p
end
''', ['-Wshort-circuit']), '')
        warning = self.check('''subroutine work(p,q)
real,pointer,intent(in)::p,q
if(associated(q).and.associated(p,q))print *,1
end subroutine
''', ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_unknown_calls_invalidate_prior_proofs(self):
        source = array_program('''if(.not.allocated(x))return
call touch()
if(allocated(x).and.size(x)>0)print *,1''',
                               extra='subroutine touch()\nend subroutine')
        warning = self.check(source, ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_loop_carried_changes(self):
        source = array_program('''if(.not.allocated(x))return
do while(allocated(x).and.size(x)>0)
call touch()
exit
end do''', extra='subroutine touch()\nend subroutine')
        warning = self.check(source, ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_block_shadowing(self):
        source = array_program('''if(.not.allocated(x))return
block
real,allocatable::x(:)
if(allocated(x).and.size(x)>0)print *,1
end block
if(allocated(x).and.size(x)>0)print *,1''')
        warning = self.check(source, ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_definitions_after_stop_are_checked(self):
        warning = self.check('''program main
stop
contains
subroutine work(x)
real,allocatable::x(:)
if(allocated(x).and.size(x)>0)print *,1
end subroutine
end program
''', ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_shadowed_inquiry_name(self):
        self.assertEqual(self.check('''program main
real::x(2)=[1.0,2.0]
if(allocated(x).and.size(x)>0)print *,1
contains
logical function allocated(a)
real,intent(in)::a(:)
allocated=size(a)>0
end function
end program
''', ['-Wshort-circuit']), '')

    def test_pure_function_preserves_prior_guard(self):
        source = array_program('''if(.not.allocated(x))return
if(touch().and.allocated(x).and.size(x)>0)print *,1''',
                               extra='pure logical function touch()\ntouch=.true.\nend function')
        self.assertEqual(self.check(source, ['-Wshort-circuit']), '')
        source = source.replace('pure logical function', 'logical function')
        warning = self.check(source, ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)

    def test_unreachable_guard_is_ignored(self):
        self.assertEqual(self.check(array_program('''if(.false.)then
if(allocated(x).and.size(x)>0)print *,1
end if'''), ['-Wshort-circuit']), '')

    def test_intrinsic_assignment_proves_allocation(self):
        self.assertEqual(self.check('''real,allocatable::x(:)
x=[1.0,2.0]
if(allocated(x).and.size(x)>0)print *,x
end
''', ['-Wshort-circuit']), '')

    def test_module_renamed_descriptor_function(self):
        source = '''module helpers
contains
pure logical function inspect(a)
real,allocatable,intent(in)::a(:)
inspect=.true.
end function
end module
''' + array_program('if(allocated(x).and.ok(x))print *,1')
        source = source.replace('subroutine work(x)\n', 'subroutine work(x)\nuse helpers,only:ok=>inspect\n', 1)
        self.assertEqual(self.check(source, ['-Wshort-circuit']), '')

    def test_imported_inquiry_shadow(self):
        source = '''module helpers
contains
logical function allocated(a)
real,intent(in)::a(:)
allocated=size(a)>0
end function
end module
program main
use helpers
real::x(2)=[1.0,2.0]
if(allocated(x).and.size(x)>0)print *,1
end program
'''
        self.assertEqual(self.check(source, ['-Wshort-circuit']), '')

    def test_generic_descriptor_forwarding_is_not_assumed_to_read(self):
        self.assertEqual(self.check('''module m
interface inspect
module procedure inspect_alloc
end interface
contains
logical function inspect_alloc(a)
real,allocatable,intent(in)::a(:)
inspect_alloc=.true.
end function
subroutine work(x)
real,allocatable,intent(in)::x(:)
if(allocated(x).and.inspect(x))print *,1
end subroutine
end module
''', ['-Wshort-circuit']), '')

    def test_pointer_component(self):
        warning = self.check('''module m
type::item
real,pointer::p
end type
contains
subroutine work(x)
type(item),intent(in)::x
if(associated(x%p).and.x%p>0)print *,1
end subroutine
end module
''', ['-Werror=short-circuit'], errors=True)
        self.assertEqual(warning.count('[short-circuit]'), 1, warning)
        self.assertIn("'x%p'", warning)

    def test_optional_warning_remains_separate(self):
        source = '''program main
call work(1.0)
contains
subroutine work(x)
real,optional,intent(in)::x
if(present(x).and.x>0)print *,1
end subroutine
end program
'''
        self.assertEqual(self.check(source, ['-Wshort-circuit']), '')
        self.assertIn('[optional-argument]', self.check(source, ['-Woptional-argument']))

    def test_module_interface_and_multifile_locations(self):
        with tempfile.TemporaryDirectory(prefix='short-circuit-') as tmp:
            directory = Path(tmp)
            module = directory/'module.f90'
            main = directory/'main.f90'
            module.write_text('''module m
contains
subroutine work(x)
real,allocatable::x(:)
if(allocated(x).and.size(x)>0)print *,1
end subroutine
end module
''')
            main.write_text('program main\nuse m\nend program\n')
            result = subprocess.run([COMPILER, '-Werror=short-circuit', '-c', module, main],
                                    cwd=directory, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('module.f90:5:26', result.stderr)
            self.assertEqual(result.stderr.count('[short-circuit]'), 1, result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
