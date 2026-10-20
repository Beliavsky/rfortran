"""Source-level implicit typing diagnostics; run after cargo build --bin rfort."""
import unittest
from pathlib import Path
import subprocess
import tempfile
import test_unused_warnings as shared
from test_record_read import COMPILER, ROOT


class ImplicitWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_first_use_and_controls(self):
        source = (ROOT/'tests/programs/ximplicit.f90').read_text(encoding='utf-8')
        source = source.replace('print *, i', 'print *, i,i')
        self.assertEqual(self.check(source), '')
        for flag in ['--wall', '-Wimplicit-typing']:
            warning = self.check(source, [flag])
            self.assertEqual(warning.count('[implicit-typing]'), 1, warning)
            self.assertIn('variable `i` is implicitly typed as INTEGER(4)', warning)
            self.assertIn('example.f90:2:1', warning)
        self.assertEqual(self.check(source, ['--wall', '-Wno-implicit-typing']), '')
        self.assertEqual(self.check(source, ['-Wimplicit-typing', '-Wno-implicit-typing']), '')
        self.assertIn('[implicit-typing]', self.check(source, ['-Wno-implicit-typing', '-Wimplicit-typing']))
        self.assertIn('[implicit-typing]', self.check(source, ['--wall', '-Wno-unused']))

    def test_errors_and_directives(self):
        source = 'i=3\nprint *,i\nend\n'
        self.assertEqual(self.check(source, ['-Werror']), '')
        for flags in [['-Werror=implicit-typing'], ['--wall', '-Werror']]:
            self.assertIn('error: variable `i` is implicitly typed', self.check(source, flags, errors=True))
        self.assertEqual(self.check('!#allow(implicit-typing)\n'+source, ['--wall']), '')
        self.assertIn('[implicit-typing]', self.check('!#deny(implicit-typing)\n'+source, errors=True))
        warning = self.check('implicit none\n'+source, ['--wall'], errors=True)
        self.assertNotIn('[implicit-typing]', warning)

    def test_default_and_custom_types(self):
        warning = self.check('implicit double precision (a-h,o-z)\ni=2\nx=3\nprint *,i,x\nend\n', ['-Wimplicit-typing'])
        self.assertIn('`i` is implicitly typed as INTEGER(4)', warning)
        self.assertIn('`x` is implicitly typed as REAL(8)', warning)
        self.assertEqual(warning.count('[implicit-typing]'), 2, warning)

    def test_explicit_and_procedure_names(self):
        self.assertEqual(self.check('''program main
integer :: i
real :: x
i=3
x=sqrt(real(i))
call work(x)
print *,x
contains
subroutine work(x)
real :: x
x=x+1
end subroutine
end program
''', ['--wall']), '')

    def test_host_and_independent_external_scope(self):
        warning = self.check('''program main
implicit none
call work()
end program
subroutine work()
i=3
print *,i
end subroutine
''', ['-Wimplicit-typing'])
        self.assertEqual(warning.count('[implicit-typing]'), 1, warning)
        self.assertIn('example.f90:6:1', warning)
        warning = self.check('''program main
i=3
call work()
contains
subroutine work()
print *,i
end subroutine
end program
''', ['-Wimplicit-typing'], errors=True)  # Existing implicit host capture limitation.
        self.assertEqual(warning.count('[implicit-typing]'), 1, warning)

    def test_loop_and_implied_do_indices(self):
        warning = self.check('''program main
do i=1,2
print *,i
end do
print *, (j,j=1,2)
end program
''', ['-Wimplicit-typing'], errors=True)  # Existing implicit I/O index limitation.
        self.assertEqual(warning.count('[implicit-typing]'), 2, warning)
        self.assertIn('`i`', warning)
        self.assertIn('`j`', warning)

    def test_module_import_and_components(self):
        self.assertEqual(self.check('''module m
integer :: n=3
type :: record
integer :: field
end type
end module
program main
use m, only: count=>n, record
type(record) :: object
object%field=count
print *, object%field
end program
''', ['-Wimplicit-typing']), '')

    def test_implicit_dummy_and_result(self):
        warning = self.check('''program main
print *, twice(2)
contains
function twice(i)
twice=2*i
end function
end program
''', ['-Werror=implicit-typing'], errors=True)
        self.assertEqual(warning.count('[implicit-typing]'), 2, warning)
        self.assertIn('`i` is implicitly typed as INTEGER(4)', warning)
        self.assertIn('`twice` is implicitly typed as REAL(4)', warning)

    def test_block_and_local_implicit_mapping(self):
        warning = self.check('''program main
implicit real (i-n)
integer :: i
i=1
block
integer :: j
j=i
print *,j
end block
call work()
contains
subroutine work()
implicit integer (i-n)
j=3
print *,j,i
end subroutine
end program
''', ['-Wimplicit-typing'])
        self.assertEqual(warning.count('[implicit-typing]'), 1, warning)
        self.assertIn('`j` is implicitly typed as INTEGER(4)', warning)
        self.assertIn('example.f90:14:1', warning)

    def test_separately_compiled_module(self):
        with tempfile.TemporaryDirectory(prefix='implicit-modules-') as tmp:
            root = Path(tmp)
            module = root/'m.f90'
            module.write_text('module m\ninteger :: n=3\nend module\n', encoding='utf-8')
            result = subprocess.run([COMPILER, '-c', module], cwd=root, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            main = root/'main.f90'
            main.write_text('use m\nprint *,n\nend\n', encoding='utf-8')
            result = subprocess.run([COMPILER, '-Wimplicit-typing', 'check', main],
                                    cwd=root, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertNotIn('[implicit-typing]', result.stderr)

    def test_keyword_value_location(self):
        warning = self.check('call system_clock(count=count)\nprint *,count\nend\n', ['-Wimplicit-typing'], errors=True)
        # COUNT is implicitly REAL, rejected by SYSTEM_CLOCK's INTEGER constraint.
        self.assertEqual(warning.count('[implicit-typing]'), 1, warning)
        self.assertIn('example.f90:1:25', warning)


if __name__ == '__main__':
    unittest.main()
