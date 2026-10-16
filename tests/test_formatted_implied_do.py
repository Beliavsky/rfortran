"""Formatted implied-DO output with runtime bounds, groups and copied values."""
import unittest
import test_deallocate


class FormattedImpliedDo(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

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
