"""Formatted implied-DO output with runtime bounds, groups and copied values."""
import unittest
import test_deallocate
from test_record_read import ROOT


class FormattedImpliedDo(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_xacv_mixed_local_and_dummy_arrays(self):
        source=(ROOT/'tests/programs/xacv_output_implied_do_bound.f90').read_text()
        variants={
            'original':source,
            'fixed':source.replace('allocate(local(n))','allocate(local(2))'),
            'print':source.replace("write(*,'(100(a,1x))')", "print '(100(a,1x))',"),
            'runtime_format':source.replace('subroutine show(words)', 'subroutine show(words,fmt)')
                .replace('integer::i,n','character(*),intent(in)::fmt\ninteger::i,n')
                .replace("write(*,'(100(a,1x))')",'write(*,fmt)')
                .replace("call show(['min','max'])", "call show(['min','max'],'(100(a,1x))')"),
        }
        for name,program in variants.items():
            with self.subTest(variant=name):
                self.check(program,'min max min max\n')

    def test_heap_array_bounds_negative_stride_and_empty(self):
        source=(ROOT/'tests/programs/xacv_output_implied_do_bound.f90').read_text()
        self.check(source.replace('allocate(local(n))','allocate(local(-1:n-2))')
                   .replace('i=1,size(local)','i=lbound(local,1),ubound(local,1)'),
                   'min max min max\n')
        self.check(source.replace('i=1,size(local)','i=size(local),1,-1'),
                   'max min min max\n')
        self.check(source.replace('i=1,size(local)','i=1,size(local),size(words)-1'),
                   'min max min max\n')
        self.check(source.replace("call show(['min','max'])",'call show([character(3)::])'),
                   '\n')

    def test_numeric_and_logical_heap_array_inquiry_bounds(self):
        for kind,values,fmt,expected in [
            ('integer(1)','[1_1,2_1]','100(i0,1x)','1 2 1 2\n'),
            ('integer(8)','[1_8,2_8]','100(i0,1x)','1 2 1 2\n'),
            ('real','[1.,2.]','100(f0.1,1x)','1.0 2.0 1.0 2.0\n'),
            ('double precision','[1d0,2d0]','100(f0.1,1x)','1.0 2.0 1.0 2.0\n'),
            ('logical','[.true.,.false.]','100(l1,1x)','T F T F\n'),
        ]:
            with self.subTest(kind=kind):
                self.check(f'''module inquiry_output_mod
implicit none
contains
subroutine show(x)
{kind},intent(in)::x(:)
{kind},allocatable::local(:)
integer::i
allocate(local(size(x)))
local=x
write(*,'({fmt})') (local(i),i=1,size(local)),(x(i),i=1,size(x))
end subroutine
end module
program main
use inquiry_output_mod
implicit none
call show({values})
end
''',expected)

    def test_mixed_output_preserves_allocation_guard(self):
        source=(ROOT/'tests/programs/xacv_output_implied_do_bound.f90').read_text()
        source=source.replace('subroutine show(words)','subroutine show(words,make)')
        source=source.replace('integer::i,n','logical,intent(in)::make\ninteger::i,n')
        source=source.replace('allocate(local(n))','if(make) allocate(local(n))')
        source=source.replace('local=words','if(make) local=words')
        source=source.replace("call show(['min','max'])",
                              "integer::flag\nread *,flag\ncall show(['min','max'],flag>0)")
        self.check(source,'min max min max\n',data='1\n')
        self.check(source,error='unallocated array',data='0\n',reference=False)

    def test_invalid_bound_reports_write_line(self):
        self.reject("implicit none\ninteger::i\nreal::n\nn=2.0\nwrite(*,'(100i0)') (i,i=1,n)\nend\n",
                    'example.f90:5:1')

    def test_character_example(self):
        self.check('''implicit none
integer::i
integer,parameter::nlen=5
character(nlen)::names(3)=[character(len=nlen)::'alice','bob','cathy']
print "(*(1x,a))", (trim(names(i)),i=1,size(names))
print "(*(1x,a))", 'alice','bob','cathy'
end
''','alice bob cathy\nalice bob cathy\n')

    def test_runtime_bounds_nested_loops_and_formats(self):
        self.check('''integer::i,j,lo,hi,step
character(40)::fmt
lo=3;hi=1;step=-1
fmt='(*(i0,1x))'
write(*,fmt=fmt) (i,i=lo,hi,step)
fmt='("values:",*(1x,i0))'
write(*,fmt) ((i*10+j,j=1,2),i=1,2)
print '(2(i0,1x,a,1x))', (i,'x',i=1,2)
print *,i,j
end
''','3 2 1\nvalues: 11 12 21 22\n1 x 2 x\n3 3\n')

    def test_copied_character_values_and_nonadvancing(self):
        self.check('''integer::i
character(4)::text
text='abcd'
write(*,'(*(a))',advance='no') (text(1:i),i=1,3)
write(*,'(a)') 'end'
print '(2(a))', (text(i:i+1),i=1,2)
end
''','aababcend\nabbc\n')

    def test_empty_loop_and_evaluated_controls(self):
        self.check('''integer::i,j,n
n=3;i=7
print '("empty",*(1x,i0))', (i,i=3,1)
print '("start",*(1x,i0))', (i,i=i,9)
print '("nested",*(1x,i0))', ((j,j=1,i),i=1,n)
print *,i,j
end
''','empty\nstart 3 4 5 6 7 8 9\nnested 1 1 2 1 2 3\n4 4\n')

    def test_mixed_values_and_scalar_items(self):
        self.check('''integer::i
print '(a,2(i0,1x,f4.1,1x,l1,1x))', 'items:', (i,real(i),(i==2),i=1,2)
end
''','items:1  1.0 F 2  2.0 T\n')

    def test_runtime_mixed_types_and_surplus_descriptors(self):
        self.check('''implicit none
integer :: i,n
n=2
write (*,'(100(i0,1x,f4.1,1x,l1,1x,a,1x))') (i,real(i),(i==2),'x',i=1,n)
end
''','1  1.0 F x 2  2.0 T x\n')

    def test_typed_constructor_lengths(self):
        self.check('''integer::i
character(5)::words(2)=[character(2)::'abc','x']
print '(*(a))', ('['//words(i)//']',i=1,2)
end
''','[ab   ][x    ]\n')

    def test_runtime_errors_and_uninitialized_items(self):
        self.check("integer::i,step\nstep=0\nprint '(*(i0))',(i,i=1,3,step)\nend\n",
                   error='step must not be zero')
        self.check("integer::i,a(2),n\na(1)=3;n=2\nprint '(*(i0))',(a(i),i=1,n)\nend\n",
                   error='uninitialized variable `a`')
        self.check("integer::i\nprint '(10000(10000x))',(i,i=1,0)\nend\n",
                   error='output format expansion exceeds supported limits')

    def test_invalid_controls_and_groups(self):
        self.reject("integer::i\nprint '(*(i0))',(i,i=1,3,0)\nend",'step must not be zero')
        for fmt in ['(*(1x))','(*(i0),a)','(2(*(i0)))','(*(i0)']:
            with self.subTest(fmt=fmt):
                self.reject(f"integer::i\nprint '{fmt}',(i,i=1,3)\nend",'output')


if __name__=='__main__':
    unittest.main(verbosity=2)
