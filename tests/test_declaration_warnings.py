"""Declaration advice: implicit SAVE and optional missing INTENT."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_unused_warnings as shared
from test_record_read import COMPILER
from test_record_read import ROOT


class DeclarationWarnings(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_implicit_save_controls_and_location(self):
        source = '''program main
call work()
contains
subroutine work()
integer :: counter = 1
print *,counter
end subroutine
end program
'''
        self.assertEqual(self.check(source), '')
        for flags in (['-Wimplicit-save'], ['--wall']):
            warning = self.check(source, flags)
            self.assertEqual(warning.count('[implicit-save]'), 1, warning)
            self.assertIn('example.f90:5:12', warning)
            self.assertIn('explicit SAVE', warning)
        self.assertEqual(self.check(source, ['--wall', '-Wno-implicit-save']), '')
        self.assertEqual(self.check(source, ['-Wimplicit-save', '-Wno-implicit-save']), '')
        self.assertIn('[implicit-save]', self.check(source, ['-Wno-implicit-save', '-Wimplicit-save']))
        for flags in (['-Werror=implicit-save'], ['-Wimplicit-save', '-Werror']):
            self.assertIn('error: local variable', self.check(source, flags, errors=True))
        self.assertEqual(self.check('!#allow(implicit-save)\n'+source, ['--wall']), '')
        self.assertIn('[implicit-save]', self.check('!#deny(implicit-save)\n'+source, errors=True))

    def test_explicit_save_constants_and_component_defaults(self):
        for save in ('integer,save::x=1', 'integer::x=1\nsave x',
                     'integer::x=1\nsave', 'save x\ninteger::x=1'):
            with self.subTest(save=save):
                self.assertEqual(self.check(f'''subroutine work()
{save}
print *,x
end subroutine
''', ['-Wimplicit-save']), '')
        self.assertEqual(self.check('''module m
integer::global=1
type::item
integer::value=2
end type
contains
subroutine work()
integer,parameter::n=3
print *,n
end subroutine
end module
program main
integer::x=5
print *,x
end program
''', ['-Wimplicit-save']), '')

    def test_initialized_entities_arrays_and_continuations(self):
        warning = self.check('''subroutine work()
integer :: a=1, b, c(2)=[2,3], &
 d=4
b=a
print *,a,b,c,d
end subroutine
''', ['-Wimplicit-save'])
        self.assertEqual(warning.count('[implicit-save]'), 3, warning)
        for name in ('a', 'c', 'd'):
            self.assertIn('local variable `'+name+'`', warning)
        self.assertNotIn('local variable `b`', warning)

    def test_pointer_initializers(self):
        warning = self.check('''module m
integer,target::target=1
contains
subroutine work()
integer,pointer::p=>null(),q=>target,r
nullify(r)
print *,associated(p),associated(q),associated(r)
end subroutine
end module
''', ['-Wimplicit-save'])
        self.assertEqual(warning.count('[implicit-save]'), 2, warning)
        self.assertIn('local variable `p`', warning)
        self.assertIn('local variable `q`', warning)
        self.assertNotIn('local variable `r`', warning)
        # Persistent pointer lowering is not supported yet; no implicit-SAVE
        # advice should accompany the unrelated backend rejection.
        warning = self.check('''subroutine work()
integer,pointer,save::p=>null()
print *,associated(p)
end subroutine
''', ['-Wimplicit-save'], errors=True)
        self.assertNotIn('[implicit-save]', warning)

    def test_no_initializer_and_scope_boundaries(self):
        self.assertEqual(self.check('''subroutine first()
integer::x
end subroutine
subroutine second()
integer::x
x=3
print *,x
end subroutine
''', ['-Wimplicit-save']), '')

    def test_nested_and_block_scopes(self):
        warning = self.check('''subroutine outer()
integer::x=1
print *,x
block
integer::x=2
print *,x
end block
call inner()
contains
subroutine inner()
integer::x=3
print *,x
end subroutine
end subroutine
''', ['-Wimplicit-save'])
        self.assertEqual(warning.count('[implicit-save]'), 3, warning)

    def test_missing_intent_opt_in_controls(self):
        source = '''subroutine work(x)
integer :: x
print *,x
end subroutine
'''
        self.assertEqual(self.check(source), '')
        self.assertEqual(self.check(source, ['--wall']), '')
        warning = self.check(source, ['-Wmissing-intent'])
        self.assertEqual(warning.count('[missing-intent]'), 1, warning)
        self.assertIn('example.f90:2:12', warning)
        self.assertIn('INTENT(OUT) makes the argument undefined', warning)
        self.assertEqual(self.check(source, ['-Wmissing-intent', '-Wno-missing-intent']), '')
        self.assertIn('[missing-intent]', self.check(source, ['-Werror=missing-intent'], errors=True))
        self.assertEqual(self.check('!#allow(missing-intent)\n'+source, ['-Wmissing-intent']), '')
        self.assertIn('[missing-intent]', self.check('!#deny(missing-intent)\n'+source, errors=True))

    def test_missing_intent_declarations_and_exemptions(self):
        self.assertEqual(self.check('''subroutine work(a,b,c)
integer,intent(in)::a
integer::b,c
intent(out)::b
intent(in out)::c
b=a
c=c+a
end subroutine
''', ['-Wmissing-intent']), '')
        self.assertEqual(self.check('''subroutine work(x,p,f)
integer,value::x
integer,pointer::p
external f
print *,x,associated(p)
call f()
end subroutine
''', ['-Wmissing-intent']), '')
        warning = self.check('''subroutine work(x,a)
real,optional::x
real::a(:)
if(present(x)) print *,x
print *,a
end subroutine
''', ['-Wmissing-intent'])
        self.assertEqual(warning.count('[missing-intent]'), 2, warning)

    def test_derived_type_dummy_intent(self):
        self.assertEqual(self.check('''module m
type::item
integer::n
end type
contains
subroutine work(x)
type(item),intent(in)::x
print *,x%n
end subroutine
end module
''', ['-Wmissing-intent']), '')

    def test_inferred_procedure_dummy(self):
        self.assertEqual(self.check('''program main
call work(show)
contains
subroutine show()
print *,1
end subroutine
subroutine work(f)
call f()
end subroutine
end program
''', ['-Wmissing-intent']), '')

    def test_example_and_runtime_behavior(self):
        source = (ROOT/'tests/programs/xdeclaration_warnings.f90').read_text()
        warning = self.check(source, ['--wall', '-Wmissing-intent'])
        self.assertEqual(warning.count('[implicit-save]'), 1, warning)
        self.assertEqual(warning.count('[missing-intent]'), 1, warning)
        import test_deallocate
        test_deallocate.Deallocate.check(self, source, '1\n3\n')

    def test_multifile_source_locations(self):
        with tempfile.TemporaryDirectory(prefix='declaration-warnings-') as tmp:
            directory = Path(tmp)
            module = directory/'module.f90'
            main = directory/'main.f90'
            module.write_text('module m\ncontains\nsubroutine work(x)\ninteger::x\ninteger::n=1\nx=n\nend subroutine\nend module\n')
            main.write_text('program main\nuse m\ninteger::x\ncall work(x)\nprint *,x\nend program\n')
            result = subprocess.run([COMPILER, '-Werror=implicit-save', '-Werror=missing-intent',
                                     '-c', module, main], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertIn('module.f90:4:10', result.stderr)
            self.assertIn('module.f90:5:10', result.stderr)
            self.assertEqual(result.stderr.count('[implicit-save]'), 1, result.stderr)
            self.assertEqual(result.stderr.count('[missing-intent]'), 1, result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
