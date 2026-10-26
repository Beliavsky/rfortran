"""Native scalar C interoperability in both directions (O0 and O2)."""
import ctypes
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT
from run_ofort_compat import equivalent

C_COMPILER = shutil.which('clang') if os.name == 'nt' else (shutil.which('cc') or shutil.which('gcc'))


class CInterop(unittest.TestCase):
    def command(self, cmd, directory, expected=0):
        result = subprocess.run(list(map(str, cmd)), cwd=directory, capture_output=True,
                                text=True, timeout=90)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def mixed(self, fortran, c, expected=''):
        if not C_COMPILER:
            self.skipTest('a C compiler is required')
        with tempfile.TemporaryDirectory(prefix='rfort-c-') as temp:
            d = Path(temp)
            (d/'test.f90').write_text(fortran)
            (d/'test.c').write_text(c)
            flags = ['--target=x86_64-pc-windows-msvc'] if os.name == 'nt' and 'clang' in Path(C_COMPILER).name else []
            self.command([C_COMPILER, *flags, '-c', 'test.c', '-o', 'support_c.obj'], d)
            for opt in (0, 2):
                with self.subTest(opt=opt):
                    p = self.command([COMPILER, '-O', opt, 'test.f90', 'support_c.obj', '-o', 'program.exe', '--run'], d)
                    self.assertTrue(equivalent(p.stdout, expected), p.stdout)
                    self.assertEqual(p.stderr, '')

    def test_import_value_and_reference(self):
        self.mixed('''program test
use, intrinsic :: iso_c_binding
implicit none
interface
 function add(i,x) result(r) bind(c,name="AddMixedC")
 import :: c_int,c_float,c_double
 integer(c_int),value :: i
 real(c_float),value :: x
 real(c_double) :: r
 end function
 subroutine bump(x,y) bind(c,name="bump_values_c")
 import :: c_int,c_double
 integer(c_int),intent(inout) :: x
 real(c_double),intent(out) :: y
 end subroutine
end interface
integer(c_int)::x
real(c_double)::y
x=5
print *,add(x,2.5_c_float)
call bump(x,y)
print *,x,y
end
''', '''double AddMixedC(int i,float x) {return i+x;}
void bump_values_c(int *x,double *y) {*x+=2;*y=3.5;}
''', '7.5\n7 3.5\n')

    def test_export_mixed_and_value_copy(self):
        self.mixed((ROOT/'tests/c_interop/scalar.f90').read_text(),
                   (ROOT/'tests/c_interop/caller.c').read_text())

    def test_scalar_widths(self):
        cases = [('c_int8_t', 'signed char', '-100'), ('c_int16_t', 'short', '-10000'),
                 ('c_int', 'int', '-1000000'), ('c_int64_t', 'long long', '-10000000000LL'),
                 ('c_float', 'float', '1.25f'), ('c_double', 'double', '1.25')]
        for kind, ctype, value in cases:
            with self.subTest(kind=kind):
                ftype = 'real' if kind in ('c_float', 'c_double') else 'integer'
                literal = value.replace('LL', '').replace('f', '')
                self.mixed(f'''function echo(x) result(r) bind(c)
use iso_c_binding
implicit none
{ftype}({kind}),value::x
{ftype}({kind})::r
r=x
end function
''', f'''extern {ctype} echo({ctype});
int main(void) {{ return echo(({ctype}){value})==({ctype}){value} ? 0 : 1; }}
''')
                self.mixed(f'''program test
use iso_c_binding
implicit none
interface
function echo(x) result(r) bind(c,name="c_echo")
use iso_c_binding
{ftype}({kind}),value::x
{ftype}({kind})::r
end function
end interface
print *,echo({literal}_{kind})
end
''', f'{ctype} c_echo({ctype} x) {{return x;}}\n', literal+'\n')

    def test_numeric_kind_constants(self):
        source = '''use iso_c_binding, only: c_int,c_long,c_size_t,c_float,c_double
implicit none
print *,c_int,c_long,c_size_t,c_float,c_double
end
'''
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'test.f90').write_text(source)
            p = self.command([COMPILER, 'run', 'test.f90'], d)
            self.assertEqual(list(map(int, p.stdout.split())),
                             [ctypes.sizeof(ctypes.c_int), ctypes.sizeof(ctypes.c_long), ctypes.sizeof(ctypes.c_size_t), 4, 8])

    def test_module_export_and_separate_compilation(self):
        source = '''module m
use iso_c_binding,only:c_int
implicit none
contains
function add_one(x) result(r) bind(c,name="SharedC")
integer(c_int),value::x
integer(c_int)::r
r=x+1
end function
end module
'''
        self.mixed(source, '''extern int SharedC(int);
int main(void) {return SharedC(4)==5 ? 0 : 1;}
''')
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'library.f90').write_text(source)
            (d/'caller.f90').write_text('use m\nimplicit none\nprint *,add_one(4)\nend\n')
            self.command([COMPILER, '-c', 'library.f90'], d)
            obj = 'library.obj' if os.name == 'nt' else 'library.o'
            p = self.command([COMPILER, 'caller.f90', obj, '-o', 'program.exe', '--run'], d)
            self.assertTrue(equivalent(p.stdout, '5\n'), p.stdout)

    def test_module_reference_function(self):
        self.mixed('''module m
use iso_c_binding
implicit none
contains
function increment(x) result(r) bind(c,name="IncrementC")
integer(c_int),intent(inout)::x
integer(c_int)::r
x=x+1
r=2*x
end function
end module
''', '''extern int IncrementC(int *);
int main(void) {
 int x=4;
 int r=IncrementC(&x);
 return x==5 && r==10 ? 0 : 1;
}
''')

    def test_fortran_value_copy(self):
        source = '''program main
use iso_c_binding
implicit none
integer(c_int)::x
x=4
call change(x)
print *,x
contains
subroutine change(i) bind(c)
integer(c_int),value::i
i=i+3
end subroutine
end program
'''
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'test.f90').write_text(source)
            for mode in (['run'], []):
                with self.subTest(mode=mode):
                    args = [COMPILER, *mode, 'test.f90'] + ([] if mode else ['-o', 'program.exe', '--run'])
                    p = self.command(args, d)
                    self.assertTrue(equivalent(p.stdout, '4\n'), p.stdout)

    def test_rejected_features_and_standard(self):
        cases = [
            ('integer,intent(in)::x(:)', 'array dummies'),
            ('integer,optional::x', 'OPTIONAL'),
            ('integer,value,intent(out)::x', 'VALUE is incompatible'),
            ('character(len=1)::x', 'not yet supported'),
            ('complex::x', 'not yet supported'),
        ]
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            for declaration, message in cases:
                with self.subTest(declaration=declaration):
                    (d/'test.f90').write_text(f'subroutine f(x) bind(c)\n{declaration}\nend subroutine\n')
                    p = subprocess.run([COMPILER, '-c', 'test.f90'], cwd=d,
                                       capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(p.returncode, 0)
                    self.assertIn(message, p.stderr)
                    self.assertIn('test.f90:1:', p.stderr)
            (d/'test.f90').write_text('subroutine f() bind(c)\nend subroutine\n')
            p = subprocess.run([COMPILER, '-std=f95', '-c', 'test.f90'], cwd=d,
                               capture_output=True, text=True, timeout=30)
            self.assertNotEqual(p.returncode, 0)
            self.assertIn('Fortran 2003', p.stderr)

    def test_interpreter_explains_native_requirement(self):
        source = '''program main
implicit none
interface
function f(x) bind(c)
integer,value::x
integer::f
end function
end interface
print *,f(1)
end program
'''
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'test.f90').write_text(source)
            p = subprocess.run([COMPILER, 'run', 'test.f90'], cwd=d,
                               capture_output=True, text=True, timeout=30)
            self.assertNotEqual(p.returncode, 0)
            self.assertIn('requires native compilation', p.stderr)

    def test_interface_diagnostic_keeps_crlf_locations(self):
        source = '''program main
use iso_c_binding
interface
function f(x) bind(c)
import :: c_int
integer(c_int)::x(:),f
end function
end interface
end program
'''
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'test.f90').write_bytes(source.replace('\n', '\r\n').encode())
            p = subprocess.run([COMPILER, '-c', 'test.f90'], cwd=d,
                               capture_output=True, text=True, timeout=30)
            self.assertNotEqual(p.returncode, 0)
            self.assertIn('array dummies', p.stderr)
            self.assertIn('test.f90:4:', p.stderr)

    def test_bind_remains_a_contextual_identifier(self):
        source = '''program main
implicit none
integer::x
x=bind(4)
call add(x)
print *,x
contains
integer function bind(i)
integer,intent(in)::i
bind=i+1
end function
subroutine add(bind)
integer,intent(inout)::bind
bind=bind+1
end subroutine
end program
'''
        with tempfile.TemporaryDirectory() as temp:
            d = Path(temp)
            (d/'test.f90').write_text(source)
            for mode in (['run'], []):
                args = [COMPILER, *mode, 'test.f90'] + ([] if mode else ['-o', 'program.exe', '--run'])
                p = self.command(args, d)
                self.assertTrue(equivalent(p.stdout, '6\n'), p.stdout)


if __name__ == '__main__':
    unittest.main()

