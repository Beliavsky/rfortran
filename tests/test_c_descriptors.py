"""Numeric ISO_Fortran_binding.h interfaces at O0 and O2."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_c_interop as interop
from test_record_read import COMPILER, ROOT

class Descriptors(unittest.TestCase):
    command = interop.CInterop.command
    mixed = interop.CInterop.mixed

    def test_example(self):
        self.mixed((ROOT/'tests/c_interop/descriptors.f90').read_text(),
                   (ROOT/'tests/c_interop/descriptor_caller.c').read_text())

    def test_numeric_widths_both_directions(self):
        for kind, ctype, tag in [('c_int8_t','signed char','int8_t'),('c_int16_t','short','int16_t'),
                                 ('c_int','int','int'),('c_int64_t','long long','int64_t'),
                                 ('c_float','float','float'),('c_double','double','double')]:
            with self.subTest(kind=kind):
                ftype='real' if kind in ('c_float','c_double') else 'integer'
                self.mixed(f'''subroutine twice(x) bind(c)
use iso_c_binding
implicit none
{ftype}({kind}),intent(inout)::x(:)
integer::j
do j=1,size(x)
 x(j)=2*x(j)
end do
end subroutine
''',f'''#include <ISO_Fortran_binding.h>
extern void twice(CFI_cdesc_t*);
int main(void) {{ {ctype} x[4]={{-2,4,7,99}};
CFI_CDESC_T(1) storage; CFI_cdesc_t *d=(CFI_cdesc_t*)&storage;
CFI_index_t shape[1]={{3}};
if(CFI_establish(d,x,CFI_attribute_other,CFI_type_{tag},sizeof({ctype}),1,shape))return 2;
twice(d);return x[0]==-4&&x[1]==8&&x[2]==14&&x[3]==99?0:1;}}
''')
                self.mixed(f'''program test
use iso_c_binding
implicit none
interface
subroutine twice(x) bind(c,name="c_twice")
use iso_c_binding
{ftype}({kind}),intent(inout)::x(:)
end subroutine
end interface
{ftype}({kind})::x(3)
x=[-2,4,7]
call twice(x)
print *,x
end
''',f'''#include <ISO_Fortran_binding.h>
void c_twice(CFI_cdesc_t *d) {{
for(CFI_index_t i=0;i<d->dim[0].extent;++i) {{
CFI_index_t sub[1]={{i}};{ctype} *p=({ctype}*)CFI_address(d,sub);*p*=2;
}} }}
''','-4 8 14\n')

    def test_strided_and_reversed_c_actual(self):
        self.mixed('''subroutine change(x) bind(c)
use iso_c_binding
implicit none
real(c_float),intent(inout)::x(0:,-1:)
integer::i,j
if (size(x,1)/=2 .or. size(x,2)/=2) stop 1
if (lbound(x,1)/=0 .or. lbound(x,2)/=-1) stop 2
do j=-1,0
 do i=0,1
  x(i,j)=x(i,j)+10*i+100*j
 end do
end do
end subroutine
''','''#include <ISO_Fortran_binding.h>
extern void change(CFI_cdesc_t*);
int main(void) {float x[8]={1,2,3,4,5,6,7,8};
CFI_CDESC_T(2) storage;CFI_cdesc_t *d=(CFI_cdesc_t*)&storage;
CFI_index_t shape[2]={2,2};
if(CFI_establish(d,x+6,CFI_attribute_other,CFI_type_float,4,2,shape))return 2;
d->dim[0].sm=-8;d->dim[1].sm=-16;
d->dim[0].lower_bound=20;d->dim[1].lower_bound=-7;
change(d);float want[8]={11,2,3,4,-85,6,-93,8};
for(int i=0;i<8;++i)if(x[i]!=want[i])return 1;return 0;}
''')

    def test_import_shape_and_output(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
subroutine fill(x) bind(c,name="c_fill")
use iso_c_binding
real(c_double),intent(out)::x(:,:)
end subroutine
end interface
real(c_double),allocatable::x(:,:)
allocate(x(2,3))
call fill(x)
print *,x
end
''','''#include <ISO_Fortran_binding.h>
void c_fill(CFI_cdesc_t *d) {
if(d->rank!=2||d->dim[0].extent!=2||d->dim[1].extent!=3)return;
for(CFI_index_t j=0;j<3;++j)for(CFI_index_t i=0;i<2;++i){
CFI_index_t sub[2]={i,j};*(double*)CFI_address(d,sub)=1+i+10*j;
}}
''','1 2 11 12 21 22\n')

    def test_helpers_and_empty_arrays(self):
        self.mixed('''subroutine empty(x) bind(c)
use iso_c_binding
implicit none
integer(c_int),intent(inout)::x(:,:)
if(size(x)/=0) stop 1
x=x+1
end subroutine
''','''#include <ISO_Fortran_binding.h>
extern void empty(CFI_cdesc_t*);
int main(void) {
CFI_CDESC_T(2) storage;CFI_cdesc_t *d=(CFI_cdesc_t*)&storage;
CFI_index_t shape[2]={2,0};
if(CFI_establish(d,0,CFI_attribute_other,CFI_type_int,99,2,shape))return 1;
if(d->elem_len!=sizeof(int)||!CFI_is_contiguous(d))return 2;
empty(d);
int x[6]={0};shape[1]=3;
if(CFI_establish(d,x,CFI_attribute_other,CFI_type_int,0,2,shape))return 3;
CFI_index_t sub[2]={1,2};if(CFI_address(d,sub)!=&x[5])return 4;
d->dim[1].sm=-8;if(CFI_is_contiguous(d))return 5;
return 0;}
''')

    def test_strided_fortran_actual_and_scalar_result(self):
        self.mixed('''program test
use iso_c_binding
implicit none
interface
function inspect(x) result(status) bind(c,name="inspect_matrix")
use iso_c_binding
real(c_float),intent(inout)::x(:,:)
integer(c_int)::status
end function
end interface
real(c_float)::x(4,3)
x=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[4,3])
print *,inspect(x(4:2:-2,3:1:-2))
print *,x
end
''','''#include <ISO_Fortran_binding.h>
int inspect_matrix(CFI_cdesc_t *d) {
if(d->rank!=2||d->dim[0].extent!=2||d->dim[1].extent!=2)return 1;
if(d->dim[0].sm!=-8||d->dim[1].sm!=-32||CFI_is_contiguous(d))return 2;
for(CFI_index_t j=0;j<2;++j)for(CFI_index_t i=0;i<2;++i){
CFI_index_t sub[2]={i,j};*(float*)CFI_address(d,sub)+=100;
}return 0;}
''','0\n1 102 3 104 5 6 7 8 9 110 11 112\n')

    def test_descriptor_forwarding(self):
        self.mixed('''subroutine forward(x) bind(c)
use iso_c_binding
implicit none
real(c_float),intent(inout)::x(:,:)
interface
subroutine bump(x) bind(c,name="c_bump_descriptor")
use iso_c_binding
real(c_float),intent(inout)::x(:,:)
end subroutine
end interface
call bump(x)
end subroutine
''','''#include <ISO_Fortran_binding.h>
extern void forward(CFI_cdesc_t*);
void c_bump_descriptor(CFI_cdesc_t *d) {
for(CFI_index_t j=0;j<2;++j)for(CFI_index_t i=0;i<2;++i){
CFI_index_t sub[2]={i,j};*(float*)CFI_address(d,sub)+=1;
}}
int main(void) {float x[8]={1,2,3,4,5,6,7,8};
CFI_CDESC_T(2) s;CFI_cdesc_t *d=(CFI_cdesc_t*)&s;CFI_index_t shape[2]={2,2};
CFI_establish(d,x+6,CFI_attribute_other,CFI_type_float,4,2,shape);
d->dim[0].sm=-8;d->dim[1].sm=-16;forward(d);
for(int i=0;i<8;++i)if(x[i]!=i+1+(i%2==0))return 1;return 0;}
''')

    def test_rank_fifteen(self):
        dimensions=','.join([':']*15)
        self.mixed(f'''subroutine increment(x) bind(c)
use iso_c_binding
implicit none
integer(c_int),intent(inout)::x({dimensions})
x=x+1
end subroutine
''','''#include <ISO_Fortran_binding.h>
extern void increment(CFI_cdesc_t*);
int main(void) {int x[2]={3,4};CFI_index_t shape[15];
for(int i=0;i<15;++i)shape[i]=1;shape[14]=2;
CFI_CDESC_T(15) s;CFI_cdesc_t *d=(CFI_cdesc_t*)&s;
if(CFI_establish(d,x,CFI_attribute_other,CFI_type_int,4,15,shape))return 2;
increment(d);return x[0]==4&&x[1]==5?0:1;}
''')

    def test_c_cannot_change_descriptor_metadata(self):
        self.mixed('''program test
implicit none
interface
subroutine corrupt(x) bind(c)
integer,intent(inout)::x(:)
end subroutine
end interface
integer::x(2)
x=[1,2]
call corrupt(x)
end
''','''#include <ISO_Fortran_binding.h>
void corrupt(CFI_cdesc_t *d) {d->rank=15;}
''',status=1,error='modified a nonallocatable array descriptor')

    def test_output_section_initialization(self):
        source='''program test
implicit none
interface
subroutine fill(x) bind(c,name="fill_selected")
integer,intent(out)::x(:)
end subroutine
end interface
integer::x(6)
call fill(x(2:6:2))
print *,x(2),x(4),x(6)
end
'''
        c='''#include <ISO_Fortran_binding.h>
void fill_selected(CFI_cdesc_t *d) {
for(CFI_index_t i=0;i<d->dim[0].extent;++i){
CFI_index_t sub[1]={i};*(int*)CFI_address(d,sub)=(int)i+10;
}}
'''
        self.mixed(source,c,'10 11 12\n')
        self.mixed(source.replace('print *,x(2),x(4),x(6)','print *,x(1)'),c,
                   status=1,error='uninitialized variable')

    def test_invalid_descriptors(self):
        f='''subroutine consume(x) bind(c)
use iso_c_binding
implicit none
integer(c_int),intent(in)::x(:)
print *,x(1)
end subroutine
'''
        for mutation, error in [('d->version=0','version'),('d->rank=2','rank'),
                                ('d->type=CFI_type_float','type'),('d->elem_len=8','element size'),
                                ('d->dim[0].extent=-1','extent'),('d->dim[0].sm=0','stride'),
                                ('d->base_addr=0','null base'),
                                ('d->dim[0].extent=PTRDIFF_MAX','overflow'),
                                ('d->base_addr=(void*)UINTPTR_MAX','overflow'),
                                ('d->attribute=CFI_attribute_pointer','attribute')]:
            with self.subTest(mutation=mutation):
                self.mixed(f,'''#include <ISO_Fortran_binding.h>
extern void consume(CFI_cdesc_t*);
int main(void) {int x[2]={1,2};CFI_CDESC_T(1) s;CFI_cdesc_t *d=(CFI_cdesc_t*)&s;
CFI_index_t shape[1]={2};CFI_establish(d,x,CFI_attribute_other,CFI_type_int,4,1,shape);
'''+mutation+';consume(d);return 0;}\n',status=1,error=error)

    def test_standard_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            d=Path(tmp)
            (d/'test.f90').write_text('subroutine f(x) bind(c)\ninteger::x(:)\nx=0\nend\n')
            p=subprocess.run([COMPILER,'-std=f2008','-c','test.f90'],cwd=d,capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0)
            self.assertIn('Fortran 2018',p.stderr)
            self.command([COMPILER,'-std=f2018','-c','test.f90'],d)

    def test_input_expression(self):
        self.mixed('''program test
implicit none
interface
subroutine check(x) bind(c,name="check_expression")
real,intent(in)::x(:)
end subroutine
end interface
real::x(3)
x=[1.0,2.0,3.0]
call check(x+1.0)
call check([2.0,3.0,4.0])
end
''','''#include <ISO_Fortran_binding.h>
extern void exit(int);
void check_expression(CFI_cdesc_t *d) {
if(d->rank!=1 || d->dim[0].extent!=3)exit(2);
for(CFI_index_t i=0;i<3;++i){CFI_index_t s[1]={i};
if(*(float*)CFI_address(d,s)!=(float)i+2.0f)exit(3);}
}
''')

if __name__=='__main__':
    unittest.main()

