"""Static format/data compatibility diagnostics before runtime I/O lowering."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_unused_warnings as shared
from test_record_read import ROOT, COMPILER


class FormatWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_example_and_options(self):
        source = (ROOT/'tests/programs/xbad_format.f90').read_text(encoding='utf-8')
        self.assertEqual(self.check(source), '')
        for flag in ['--wall', '-Wformat']:
            warning = self.check(source, [flag])
            self.assertEqual(warning.count('[format]'), 1, warning)
            self.assertIn('I4 expects INTEGER, but output item 2 is REAL', warning)
            self.assertIn('example.f90:2:1', warning)
        self.assertEqual(self.check(source, ['--wall', '-Wno-format']), '')
        self.assertEqual(self.check(source, ['-Wformat', '-Wno-format']), '')
        self.assertIn('[format]', self.check(source, ['-Wno-format', '-Wformat']))
        self.assertIn('error: format descriptor', self.check(source, ['-Werror=format'], errors=True))
        self.assertIn('[format]', self.check(source, ['--wall', '-Werror'], errors=True))
        self.assertEqual(self.check('!#allow(format)\n'+source, ['--wall']), '')
        self.assertIn('[format]', self.check('!#deny(format)\n'+source, errors=True))

    def test_intrinsic_item_types(self):
        warning = self.check('''print '(i4,f8.2,l1,a)', 1.0, 2, "T", .true.
end
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 4, warning)
        for expected in ['expects INTEGER', 'expects REAL', 'expects LOGICAL', 'expects CHARACTER']:
            self.assertIn(expected, warning)
        self.assertEqual(self.check('''print '(i4,f8.2,l1,a)', 1, 2.0, .true., "ok"
print '(*(g0,1x))', 1, 2.0, .false., "ok"
end
''', ['-Wformat']), '')

    def test_parameters_labels_and_internal_write(self):
        warning = self.check('''implicit none
character(*),parameter :: fmt='(i4)'
character(20) :: buffer
write(buffer,fmt) 1.0
write(*,10) .true.
10 format(i4)
end
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 2, warning)
        self.assertIn('example.f90:4:1', warning)
        self.assertIn('example.f90:5:1', warning)

    def test_arrays_sections_and_implied_do(self):
        warning = self.check('''implicit none
integer :: i
real :: x(2,2)
x=1.
print '(i4)', x
print '(i4)', x(:,2)
print '(i4)', (real(i),i=1,2)
end
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 3, warning)  # Avoid one warning per array element.

    def test_reversion_and_unused_descriptors_are_valid(self):
        self.assertEqual(self.check('''print '(i4)', 1,2,3
print '(100i4)', 1
print '(i4,2(f6.1,1x))', 1,2.0,3.0,4.0,5.0
print '(i4,:,a)',1
end
''', ['-Wformat']), '')
        warning = self.check("print '(i4)',1,2.0\nend\n", ['-Wformat'])
        self.assertIn('output item 2 is REAL', warning)

    def test_complex_consumes_two_real_descriptors(self):
        self.assertEqual(self.check('''complex :: z
z=(1.,2.)
print '(2f6.1,i4)',z,3
end
''', ['-Wformat']), '')
        warning = self.check('''complex :: z
z=(1.,2.)
print '(f6.1,i4)',z
end
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 1, warning)
        self.assertIn('COMPLEX component 2', warning)

    def test_formatted_read_and_status_control(self):
        warning = self.check('''integer :: i,ios
character(12) :: text='1.0'
read(text,'(f6.1)') i
read(text,'(f6.1)',iostat=ios) i
end
''', ['-Werror=format'], errors=True)
        self.assertEqual(warning.count('[format]'), 2, warning)
        self.assertIn('input item 1 is INTEGER', warning)

    def test_runtime_format_is_not_assumed_constant(self):
        self.assertEqual(self.check('''character(8) :: fmt='(i4)'
fmt='(f6.1)'
print fmt,1.0
end
''', ['-Wformat']), '')

    def test_user_function_result(self):
        warning = self.check('''program main
print '(i4)',f()
contains
real function f()
f=1.
end function
end program
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 1, warning)

    def test_module_import(self):
        warning = self.check('''module m
character(*),parameter :: fmt='(i4)'
real :: x=1.
end module
program main
use m
print fmt,x
end program
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 1, warning)

    def test_unknown_array_size_preserves_only_known_prefix(self):
        warning = self.check('''program main
real :: x(2)
x=1.
call work(x)
contains
subroutine work(x)
real :: x(:)
print '(i4)',1.0,x,2.0
end subroutine
end program
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 1, warning)
        self.assertIn('output item 1 is REAL', warning)
        self.assertNotIn('output item 3', warning)

    def test_boz_does_not_reject_legal_noninteger_types(self):
        self.assertEqual(self.check('''real :: x=1.
logical :: flag=.true.
print '(z8)',x
print '(b8)',flag
end
''', ['-Wformat']), '')

    def test_formatted_read_matching_types(self):
        self.assertEqual(self.check('''integer :: i
real :: x
logical :: l
character(2) :: s
character(30) :: text='1  2.0 T ok'
read(*,'(i3,f5.1,l2,a2)') i,x,l,s
end
''', ['-Wformat']), '')

    def test_separate_module_constant_format(self):
        with tempfile.TemporaryDirectory(prefix='format-module-') as tmp:
            root = Path(tmp)
            module = root/'m.f90'
            module.write_text("module m\ncharacter(*),parameter::fmt='(i4)'\nreal::x=1.\nend module\n", encoding='utf-8')
            result = subprocess.run([COMPILER, '-c', module], cwd=root, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            main = root/'main.f90'
            main.write_text('use m\nprint fmt,x\nend\n', encoding='utf-8')
            result = subprocess.run([COMPILER, '-Wformat', 'check', main], cwd=root,
                                    capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertEqual(result.stderr.count('[format]'), 1, result.stderr)
            self.assertIn('main.f90:2:1', result.stderr)

    def test_warning_in_native_and_interpreter_modes(self):
        with tempfile.TemporaryDirectory(prefix='format-modes-') as tmp:
            source = Path(tmp)/'bad.f90'
            source.write_text('print "(i4)",1.0\nend\n', encoding='utf-8')
            for args in [['-Werror=format', 'run', source], ['-Werror=format', '-c', source]]:
                result = subprocess.run([COMPILER, *args], cwd=tmp, capture_output=True, text=True, encoding='utf-8')
                self.assertNotEqual(result.returncode, 0, result.stdout+result.stderr)
                self.assertIn('format descriptor I4 expects INTEGER', result.stderr)
                self.assertIn('format warnings treated as errors', result.stderr)

    def test_scoped_function_results_do_not_leak(self):
        warning = self.check('''program main
call one()
call two()
end program
subroutine one()
print '(i4)',f()
contains
real function f()
f=1.
end function
end subroutine
subroutine two()
print '(i4)',f()
contains
integer function f()
f=1
end function
end subroutine
''', ['-Wformat'])
        self.assertEqual(warning.count('[format]'), 1, warning)

    def test_complex_array_expression_does_not_shift_later_items(self):
        self.assertEqual(self.check('''complex :: z(2)
z=(1.,2.)
print '(4f6.1,i4)',z+(1.,0.),3
end
''', ['-Wformat']), '')


if __name__ == '__main__':
    unittest.main()
