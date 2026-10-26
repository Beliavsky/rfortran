"""Opaque C pointers, native association and stable target addresses."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_c_interop as interop
from test_record_read import COMPILER, ROOT

class CPointers(unittest.TestCase):
    command=interop.CInterop.command
    mixed=interop.CInterop.mixed

    def test_c_buffer(self):
        self.mixed((ROOT/'tests/c_interop/pointers.f90').read_text(encoding='utf-8'),
                   (ROOT/'tests/c_interop/pointer_buffer.c').read_text(encoding='utf-8'),
                   '3 1 2 3\n1\n')

    def test_null_and_associated(self):
        self.mixed('''program test
use iso_c_binding,only:c_ptr,c_null_ptr,c_associated
implicit none
type(c_ptr)::p=c_null_ptr,q
q=p
print *,c_associated(p),c_associated(p,q)
end
''','int unused;','F F\n')

    def test_c_loc_scalar_and_array(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine bump(p) bind(c)
import c_ptr
type(c_ptr),value::p
end subroutine
end interface
integer(c_int64_t),target::x
integer(c_int64_t),pointer::y
real(c_double),target::a(2)
real(c_double),pointer::b(:)
x=5
a=[1.0_c_double,2.0_c_double]
call bump(c_loc(x))
call c_f_pointer(c_loc(x),y)
y=y+2
call c_f_pointer(c_loc(a),b,[2])
b(2)=7.0_c_double
print *,x,a
end
''','void bump(void *p){*(long long*)p+=3;}','10 1 7\n')

    def test_widths_and_lower_bounds(self):
        for kind,ftype,ctype in [('c_int8_t','integer','signed char'),('c_int16_t','integer','short'),('c_int','integer','int'),('c_int64_t','integer','long long'),('c_float','real','float'),('c_double','real','double')]:
            with self.subTest(kind=kind):
                self.mixed(f'''program test
use iso_c_binding
implicit none
interface
function buffer() bind(c) result(p)
import c_ptr
type(c_ptr)::p
end function
function verify() bind(c) result(i)
integer::i
end function
end interface
type(c_ptr)::p
{ftype}({kind}),pointer::a(:,:)
integer::bounds(2),dims(2)
bounds=[-1,3]
dims=[2,3]
p=buffer()
if(c_associated(p)) then
call c_f_pointer(lower=bounds,shape=dims,fptr=a,cptr=p)
end if
if (.not.c_associated(c_loc(a),p)) stop 3
print *,lbound(a),ubound(a),a(-1,3),a(0,5)
a(-1,3)=10
a(0,5)=20
print *,verify()
end
''',f'''static {ctype} x[6]={{1,2,3,4,5,6}};
void *buffer(void){{return x;}}
int verify(void){{return x[0]==10&&x[5]==20;}}
''','-1 3 0 5 1 6\n1\n')

    def test_value_reference_and_return_both_directions(self):
        self.mixed('''function echo(p) result(q) bind(c)
use iso_c_binding
implicit none
type(c_ptr),value::p
type(c_ptr)::q
q=p
end function
subroutine replace(p,q) bind(c)
use iso_c_binding
implicit none
type(c_ptr),intent(out)::p
type(c_ptr),value::q
p=q
end subroutine
''','''extern void *echo(void*);
extern void replace(void**,void*);
int main(void){int x;void *p=0;if(echo(&x)!=&x)return 1;
replace(&p,&x);return p==&x?0:2;}
''')
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine get(p) bind(c)
import c_ptr
type(c_ptr),intent(out)::p
end subroutine
function echo(p) result(q) bind(c)
import c_ptr
type(c_ptr),value::p
type(c_ptr)::q
end function
end interface
type(c_ptr)::p,q
call get(p)
q=echo(p)
print *,c_associated(c_ptr_2=q,c_ptr_1=p)
end
''','''static int x;
void get(void **p){*p=&x;}
void *echo(void *p){return p;}
''','T\n')

    def test_aliases_and_fortran_helpers(self):
        self.mixed('''module handles
use iso_c_binding,only:handle=>c_ptr,nil=>c_null_ptr,same=>c_associated,loc=>c_loc,view=>c_f_pointer,c_double
implicit none
contains
function identity(p) result(q)
type(handle),intent(in)::p
type(handle)::q
q=p
end function
end module
program test
use handles
implicit none
type(handle)::p=nil
real(c_double),target::x
real(c_double),pointer::y
x=4
p=identity(loc(x))
call view(fptr=y,cptr=p)
y=7
print *,same(p,loc(x)),x
end
''','int unused;','T 7\n')

    def test_null_disassociation_and_dereference(self):
        source='''program test
use iso_c_binding
implicit none
real(c_double),pointer::a(:)
call c_f_pointer(c_null_ptr,a,[0])
print *,associated(a)
end
'''
        self.mixed(source,'int unused;','F\n')
        self.mixed(source.replace('print *,associated(a)','print *,a(1)'),
                   'int unused;',status=1,error='unassociated pointer')

    def test_invalid_source(self):
        cases=[('p=3','requires another C_PTR'),('print \"(z16)\",p','do not support'),('print *,[p]','not yet supported'),('i=p','requires another C_PTR'),
               ('i=abs(p)','explicit compatible procedure interface'),('if(p==c_null_ptr) stop 1','opaque'),
               ('print *,c_associated(i)','must have type C_PTR'),
               ('call c_f_pointer(p,x)','POINTER attribute'),
               ('call c_f_pointer(p,a)','requires SHAPE'),
               ('call c_f_pointer(p,a,[1.0])','INTEGER array'),
               ('p=c_loc(x)','requires TARGET'),
               ('p=c_loc(narrow)','narrower kinds'),
               ('call consume(p=3)','incompatible C_PTR')]
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            for code,error in cases:
                with self.subTest(code=code):
                    (d/'test.f90').write_text('''program test
use iso_c_binding
implicit none
interface
subroutine consume(p) bind(c)
import c_ptr
type(c_ptr),value::p
end subroutine
end interface
type(c_ptr)::p=c_null_ptr
integer::i,x
real,target::narrow
real(c_double),pointer::a(:)
'''+code+'\nend\n',encoding='utf-8')
                    p=subprocess.run([COMPILER,'-c','test.f90'],cwd=d,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                    self.assertIn(error,p.stderr)
                    self.assertIn('test.f90:',p.stderr)

    def test_rank_fifteen_and_empty(self):
        dims=','.join(':' for _ in range(15))
        self.mixed(f'''program test
use iso_c_binding
implicit none
interface
function buffer() bind(c) result(p)
import c_ptr
type(c_ptr)::p
end function
end interface
integer(c_int),pointer::a({dims})
integer::shape(15)
shape=1
shape(15)=2
call c_f_pointer(buffer(),a,shape)
a=a+1
print *,size(a),sum(a)
call c_f_pointer(buffer(),a,[0,1,1,1,1,1,1,1,1,1,1,1,1,1,1])
print *,associated(a),size(a)
end
''','static int x[2]={3,4};void *buffer(void){return x;}','2 9\nT 0\n')

    def test_standard_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            source='''program test
use iso_c_binding,only:c_null_ptr,c_f_pointer
implicit none
real,pointer::a(:)
call c_f_pointer(c_null_ptr,a,[0],[-1])
end
'''
            (d/'test.f90').write_text(source,encoding='utf-8')
            p=subprocess.run([COMPILER,'-std=f2018','-c','test.f90'],cwd=d,capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
            self.assertIn('C_F_POINTER LOWER',p.stderr)
            self.command([COMPILER,'-std=f2023','-c','test.f90'],d)

    def test_shape_errors(self):
        source='''program test
use iso_c_binding
implicit none
real(c_double),target::x
real(c_double),pointer::a(:,:)
x=1
call c_f_pointer(c_loc(x),a,SHAPE)
end
'''
        for shape,error in [('[1]','size must equal'),('[1,-1]','negative extent')]:
            with self.subTest(shape=shape):
                self.mixed(source.replace('SHAPE',shape),'int unused;',status=1,error=error)

    def test_interpreter_reports_native_requirement(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            (d/'test.f90').write_text('use iso_c_binding\nreal,pointer::a(:)\ncall c_f_pointer(c_null_ptr,a,[0])\nend\n',encoding='utf-8')
            p=subprocess.run([COMPILER,'run','test.f90'],cwd=d,capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
            self.assertIn('requires native compilation',p.stderr)

    def test_associated_evaluates_arguments_once(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
function buffer() bind(c) result(p)
import c_ptr
type(c_ptr)::p
end function
function count_calls() bind(c) result(n)
integer::n
end function
end interface
print *,c_associated(buffer(),buffer()),count_calls()
end
''','''static int n,x;
void *buffer(void){++n;return &x;}
int count_calls(void){return n;}
''','T 2\n')

    def test_separate_modules_and_module_target(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            (d/'handles.f90').write_text('''module handles
use iso_c_binding
implicit none
real(c_double),target::x=2.0_c_double
contains
function handle() result(p)
type(c_ptr)::p
p=c_loc(x)
end function
end module
''',encoding='utf-8')
            (d/'main.f90').write_text('''program test
use handles
implicit none
real(c_double),pointer::y
call c_f_pointer(handle(),y)
y=7
print *,x
end
''',encoding='utf-8')
            for opt in (0,2):
                with self.subTest(opt=opt):
                    self.command([COMPILER,'-O',opt,'-c','handles.f90'],d)
                    p=self.command([COMPILER,'-O',opt,'main.f90','handles.obj','--run'],d)
                    self.assertEqual(float(p.stdout.strip()),7.0)

    def test_c_loc_rejects_dummy_adapter_temporary(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            (d/'test.f90').write_text('''function address(v) result(p)
use iso_c_binding
implicit none
real(c_double),intent(in),target::v
type(c_ptr)::p
p=c_loc(v)
end function
''',encoding='utf-8')
            p=subprocess.run([COMPILER,'-c','test.f90'],cwd=d,capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
            self.assertIn('temporary storage',p.stderr)
            self.assertIn('test.f90:6:',p.stderr)

if __name__=='__main__':unittest.main(verbosity=2)
