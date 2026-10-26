"""Native numeric C interoperability in both directions (O0 and O2)."""
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

    def mixed(self, fortran, c, expected='', status=0, error=''):
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
                    p = self.command([COMPILER, '-O', opt, 'test.f90', 'support_c.obj', '-o', 'program.exe', '--run'], d, expected=status)
                    self.assertTrue(equivalent(p.stdout, expected), p.stdout)
                    if error:
                        self.assertIn(error, p.stderr)
                    else:
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

    def test_matrix_example(self):
        self.mixed((ROOT/'tests/c_interop/arrays.f90').read_text(),
                   (ROOT/'tests/c_interop/array_caller.c').read_text())

    def test_explicit_array_widths(self):
        cases = [('c_int8_t', 'signed char'), ('c_int16_t', 'short'),
                 ('c_int', 'int'), ('c_int64_t', 'long long'),
                 ('c_float', 'float'), ('c_double', 'double')]
        for kind, ctype in cases:
            with self.subTest(kind=kind):
                ftype = 'real' if kind in ('c_float', 'c_double') else 'integer'
                declarations = f"integer(c_int),value::n\n{ftype}({kind}),intent(in)::x(n)\n{ftype}({kind}),intent(out)::y(n)"
                self.mixed(f'''program test
use iso_c_binding
implicit none
interface
subroutine transform(n,x,y) bind(c,name="c_transform")
use iso_c_binding
{declarations}
end subroutine
end interface
{ftype}({kind})::x(3),y(3)
x=[-2,4,7]
call transform(3,x,y)
print *,y
end
''', f'void c_transform(int n,const {ctype} *x,{ctype} *y) {{for(int i=0;i<n;++i)y[i]=x[i]*2;}}', '-4 8 14\n')
                self.mixed(f'''subroutine transform(n,x,y) bind(c,name="f_transform")
use iso_c_binding
implicit none
{declarations}
integer::i
do i=1,n
 y(i)=2*x(i)
end do
end subroutine
''', f'''extern void f_transform(int,const {ctype}*,{ctype}*);
int main(void) {{ {ctype} x[3]={{-2,4,7}},y[3]={{0}};
f_transform(3,x,y);return y[0]==-4&&y[1]==8&&y[2]==14?0:1;}}
''')

    def test_assumed_size_array_widths(self):
        cases = [('c_int8_t', 'signed char'), ('c_int16_t', 'short'),
                 ('c_int', 'int'), ('c_int64_t', 'long long'),
                 ('c_float', 'float'), ('c_double', 'double')]
        for kind, ctype in cases:
            with self.subTest(kind=kind):
                ftype = 'real' if kind in ('c_float', 'c_double') else 'integer'
                self.mixed(f'''subroutine change(x,howmany) bind(c,name="f_change")
use iso_c_binding
implicit none
integer(c_int),value,intent(in)::howmany
{ftype}({kind}),intent(inout)::x(*)
integer::j
do j=1,howmany
 x(j)=2*x(j)
end do
end subroutine
''', f'''extern void f_change({ctype}*,int);
int main(void) {{{ctype} x[4]={{-2,4,7,99}};
f_change(x,3);return x[0]==-4 && x[1]==8 && x[2]==14 && x[3]==99 ? 0:1;}}
''')
                self.mixed(f'''program test
use iso_c_binding
implicit none
interface
subroutine change(howmany,x) bind(c,name="c_change")
use iso_c_binding
integer(c_int),value::howmany
{ftype}({kind}),intent(inout)::x(*)
end subroutine
end interface
{ftype}({kind})::x(4)
x=[-2,4,7,99]
call change(3,x)
print *,x
end
''', f'void c_change(int n,{ctype} *x) {{for(int i=0;i<n;++i)x[i]*=2;}}', '-4 8 14 99\n')

    def test_assumed_size_multidimensional_and_sections(self):
        self.mixed('''subroutine change(x,columns) bind(c)
use iso_c_binding
implicit none
integer(c_int),value,intent(in)::columns
real(c_float),intent(inout)::x(0:1,-2:*)
integer::i,j
do j=-2,columns-3
 do i=0,1
  x(i,j)=x(i,j)+10*i+100*j
 end do
end do
x(:, -2:-1)=x(:, -2:-1)+1
end subroutine
''', '''extern void change(float*,int);
int main(void) {float x[6]={1,2,3,4,5,6};change(x,2);
float want[6]={-198,-187,-96,-85,5,6};
for(int i=0;i<6;++i)if(x[i]!=want[i])return 1;return 0;}
''')
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine change(x,n) bind(c,name="c_change_matrix")
use iso_c_binding
integer(c_int),value::n
integer(c_int),intent(inout)::x(0:1,-2:*)
end subroutine
end interface
integer(c_int)::x(2,3)
x=reshape([1,2,3,4,5,6],[2,3])
call change(x,2)
print *,x
end
''', '''void c_change_matrix(int *x,int n) {for(int i=0;i<2*n;++i)x[i]+=10;}
''', '11 12 13 14 5 6\n')

    def test_assumed_size_function_and_output(self):
        self.mixed((ROOT/'tests/c_interop/assumed_size.f90').read_text(),
                   (ROOT/'tests/c_interop/assumed_size_caller.c').read_text())

    def test_assumed_size_rank_three_and_inquiries(self):
        self.mixed('''subroutine cube(x,last,which) bind(c)
use iso_c_binding
implicit none
integer(c_int),value,intent(in)::last,which
integer(c_int),intent(inout)::x(2,3,*)
integer::i,j,k
if (size(x,which)/=3) stop 1
if (ubound(x,1)/=2) stop 2
if (lbound(x,3)/=1) stop 3
do k=1,last
 do j=1,3
  do i=1,2
   x(i,j,k)=i+10*j+100*k
  end do
 end do
end do
end subroutine
''', '''extern void cube(int*,int,int);
int main(void) {int x[12]={0};cube(x,2,2);int k=0;
for(int z=1;z<=2;++z)for(int y=1;y<=3;++y)for(int i=1;i<=2;++i)
if(x[k++]!=i+10*y+100*z)return 1;return 0;}
''')

    def test_assumed_size_forwards_to_c(self):
        self.mixed('''subroutine forward(x,howmany) bind(c)
use iso_c_binding
implicit none
integer(c_int),value,intent(in)::howmany
real(c_float),intent(inout)::x(*)
interface
subroutine bump(x,n) bind(c,name="c_bump")
use iso_c_binding
integer(c_int),value::n
real(c_float),intent(inout)::x(*)
end subroutine
end interface
call bump(x,howmany)
end subroutine
''', '''extern void forward(float*,int);
void c_bump(float *x,int n) {for(int i=0;i<n;++i)x[i]+=0.5f;}
int main(void) {float x[3]={1,2,99};forward(x,2);
return x[0]==1.5f && x[1]==2.5f && x[2]==99 ? 0:1;}
''')

    def test_fortran_caller_of_assumed_size_binding(self):
        source = '''module m
use iso_c_binding
implicit none
contains
subroutine twice(x,howmany) bind(c)
integer(c_int),value,intent(in)::howmany
real(c_float),intent(inout)::x(*)
integer::j
do j=1,howmany
 x(j)=2*x(j)
end do
end subroutine
end module
program test
use m
implicit none
real(c_float)::x(3)
x=[1,2,99]
call twice(x,2)
print *,x
end
'''
        with tempfile.TemporaryDirectory() as temp:
            d=Path(temp)
            (d/'test.f90').write_text(source)
            for mode in (['run'], []):
                with self.subTest(mode=mode):
                    args=[COMPILER,*mode,'test.f90']+([] if mode else ['-o','program.exe','--run'])
                    p=self.command(args,d)
                    self.assertTrue(equivalent(p.stdout,'2 4 99\n'),p.stdout)

    def test_assumed_size_rejects_unknown_extent_operations(self):
        for statement in ('print *,size(x)', 'print *,size(x,2)',
                          'print *,shape(x)', 'print *,x', 'x=1', 'x(:,:)=1'):
            with self.subTest(statement=statement), tempfile.TemporaryDirectory() as temp:
                d=Path(temp)
                (d/'test.f90').write_text('subroutine invalid(x) bind(c)\ninteger::x(2,*)\n'+statement+'\nend subroutine\n')
                p=subprocess.run([COMPILER,'-c','test.f90'],cwd=d,capture_output=True,text=True,timeout=90)
                self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                self.assertIn('assumed-size',p.stderr)

    def test_multidimensional_array_order_and_lower_bounds(self):
        self.mixed('''subroutine transform(n,x) bind(c,name="matrix")
use iso_c_binding
implicit none
integer(c_int),intent(in)::n
real(c_float),intent(inout)::x(0:1,-1:n-2)
integer::i,j
do j=-1,n-2
 do i=0,1
  x(i,j)=x(i,j)+10*i+100*j
 end do
end do
end subroutine
''', '''extern void matrix(const int*,float*);
int main(void) {int n=3;float x[6]={1,2,3,4,5,6};
matrix(&n,x);float expected[6]={-99,-88,3,14,105,116};
for(int i=0;i<6;++i)if(x[i]!=expected[i])return 1;return 0;}
''')
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine matrix(n,x) bind(c,name="c_matrix")
use iso_c_binding
integer(c_int),value::n
real(c_double),intent(inout)::x(2,n)
end subroutine
end interface
real(c_double)::x(2,3)
x=reshape([1,2,3,4,5,6],[2,3])
call matrix(3,x)
print *,x
end
''', '''void c_matrix(int n,double *x) {for(int j=0;j<n;++j)for(int i=0;i<2;++i)x[i+2*j]+=10*i+100*j;}
''', '1 12 103 114 205 216\n')

    def test_array_import_result_and_allocated_actual(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
function total(n,x) result(r) bind(c,name="c_total")
use iso_c_binding
integer(c_int),value::n
real(c_double),intent(in)::x(n)
real(c_double)::r
end function
subroutine fill(n,x) bind(c,name="c_fill")
use iso_c_binding
integer(c_int),value::n
real(c_double),intent(out)::x(n)
end subroutine
end interface
real(c_double),allocatable::x(:)
allocate(x(4))
call fill(4,x)
print *,x,total(4,x)
end
''', '''void c_fill(int n,double *x) {for(int i=0;i<n;++i)x[i]=i+1;}
double c_total(int n,const double *x) {double s=0;for(int i=0;i<n;++i)s+=x[i];return s;}
''', '1 2 3 4 10\n')

    def test_rank_three_array_export(self):
        self.mixed('''subroutine cube(x) bind(c)
use iso_c_binding
implicit none
integer(c_int),intent(inout)::x(2,2,2)
integer::i,j,k
do k=1,2
 do j=1,2
  do i=1,2
   x(i,j,k)=x(i,j,k)+i+10*j+100*k
  end do
 end do
end do
end subroutine
''', '''extern void cube(int*);
int main(void) {int x[8]={0};cube(x);int k=0;
for(int z=1;z<=2;++z)for(int y=1;y<=2;++y)for(int i=1;i<=2;++i)
if(x[k++]!=i+10*y+100*z)return 1;return 0;}
''')

    def test_zero_size_arrays(self):
        self.mixed('''subroutine empty(n,x) bind(c)
use iso_c_binding
implicit none
integer(c_int),value::n
real(c_float),intent(inout)::x(n)
x=x+1
end subroutine
''', '''extern void empty(int,float*);
int main(void) {float x=42;empty(0,&x);return x==42?0:1;}
''')

    def test_array_input_initialization_guard(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine consume(x) bind(c)
use iso_c_binding
integer(c_int),intent(in)::x(2)
end subroutine
end interface
integer(c_int)::x(2)
x(1)=1
call consume(x)
end
''', 'void consume(const int *x) {(void)x;}\n',
                   status=1, error='uninitialized C array argument element')

    def test_array_allocation_overflow_guard(self):
        self.mixed('''subroutine huge(n,x) bind(c)
use iso_c_binding
implicit none
integer(c_int64_t),value::n
real(c_double),intent(inout)::x(n)
x=x+1
end subroutine
''', '''#include <limits.h>
extern void huge(long long,double*);
int main(void) {double x=0;huge(LLONG_MAX,&x);return 0;}
''', status=1, error='C array size overflow')

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
subroutine fill(n,x) bind(c,name="SharedFill")
integer(c_int),value::n
integer(c_int),intent(out)::x(n)
integer::i
do i=1,n
 x(i)=i
end do
end subroutine
end module
'''
        self.mixed(source, '''extern int SharedC(int);
extern void SharedFill(int,int*);
int main(void) {int x[3]={0};SharedFill(3,x);
return SharedC(4)==5 && x[0]==1 && x[2]==3 ? 0 : 1;}
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
            ('integer,value::x(2)', 'VALUE is not allowed'),
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

