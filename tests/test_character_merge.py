"""CHARACTER MERGE in interpreter/native O0/O2, with gfortran references."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate as shared
from test_record_read import ROOT, build_commands


class CharacterMerge(unittest.TestCase):
    check=shared.Deallocate.check
    reject=shared.Deallocate.reject

    def test_requested_random_examples(self):
        for name in ('xmerge_char.f90','xmerge_char_vec.f90','xmerge_char_vec_alloc.f90'):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                work=Path(tmp);source=work/name
                source.write_text((ROOT/'tests/programs'/name).read_text())
                for command in build_commands(source,work,cwd=work):
                    result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,'')
                    lines=result.stdout.splitlines()
                    self.assertEqual(len(lines),8 if name=='xmerge_char_vec_alloc.f90' else 5)
                    for line in lines:
                        self.assertEqual(len(line),12)
                        word,number=line.split()
                        x=float(number)
                        self.assertIn(word,('big','small'))
                        if abs(x-0.5)>0.00051:
                            self.assertEqual(word,'big' if x>0.5 else 'small')

    def test_runtime_allocation_resize_empty_and_overlap(self):
        self.check('''implicit none
integer::n
real,allocatable::x(:)
character(2),allocatable::a(:),b(:),s(:)
read *,n
allocate(x(n),a(n),b(n))
x=[1.,-1.,2.]
a=['aa','bb','cc'];b=['11','22','33']
s=merge(a,b,x>0.)
print *,allocated(s),size(s)
print '(3a2)',s
a=merge(a(3:1:-1),a,x>0.)
print '(3a2)',a
s=merge('!!',b,.false.)
print '(3a2)',s
s=merge(merge('++','--',x>0.),'??',x<0.)
print '(3a2)',s
deallocate(x,a,b)
allocate(x(1),a(1))
x=1.;a='zz'
s=merge(a,'--',x>0.)
print *,size(s)
print '(a)',s
deallocate(x,a)
allocate(x(0),a(0))
s=merge(a,'--',x>0.)
print *,allocated(s),size(s)
end
''','T 3\naa22cc\nccbbaa\n112233\n??--??\n1\nzz\nT 0\n',data='3\n')

    def test_runtime_large_arrays_and_nondefault_bounds(self):
        self.check('''implicit none
integer::n
real,allocatable::x(:)
character(2),allocatable::a(:),s(:)
read *,n
allocate(x(-2:n-3),a(0:n-1))
x=1.;a='aa'
x(-2)=-1.
s=merge(a,'bb',x>0.)
print *,size(s),lbound(s,1),ubound(s,1)
print '(2a2)',s(1),s(n)
end
''','1100 1 1100\nbbaa\n',data='1100\n')

    def test_runtime_mask_with_fixed_source(self):
        self.check('''implicit none
integer::n
real,allocatable::x(:)
character(2)::a(3)
character(2),allocatable::s(:)
read *,n
allocate(x(n))
x=[-1.,1.,-1.];a=['aa','bb','cc']
s=merge(a,'!!',x>0.)
print '(3a2)',s
end
''','!!bb!!\n',data='3\n')

    def test_runtime_shape_mismatch(self):
        self.check('''implicit none
integer::n
character(2),allocatable::a(:),b(:),s(:)
logical,allocatable::mask(:)
read *,n
allocate(a(n),b(n+1),mask(n))
a='aa';b='bb';mask=.true.
s=merge(a,b,mask)
end
''',error='MERGE argument shapes do not conform',data='3\n')
        self.check('''implicit none
integer::n
real,allocatable::a(:),b(:)
character(2),allocatable::s(:)
read *,n
allocate(a(n),b(n+1))
a=1.;b=0.
s=merge('aa','bb',a>b)
end
''',error='MERGE argument shapes do not conform',data='3\n')

    def test_runtime_scalar_mask_evaluated_once(self):
        self.check('''implicit none
integer::n,calls
character(2),allocatable::a(:),s(:)
read *,n
allocate(a(n))
a='aa';calls=0
s=merge(a,'bb',choose())
print *,calls,size(s)
print '(3a2)',s
contains
logical function choose()
calls=calls+1
choose=.true.
end function
end
''','1 3\naaaaaa\n',data='3\n')

    def test_runtime_unallocated_source(self):
        self.reject('''implicit none
character(2),allocatable::a(:),s(:)
logical,allocatable::mask(:)
allocate(mask(0))
s=merge(a,'bb',mask)
end
''','array `a` is used before ALLOCATE')

    def test_scalars_variables_keywords_and_padding(self):
        self.check('''implicit none
character(5)::a,b,s
logical::mask
a='yes';b='no';mask=.false.
s=merge(a,b,mask)
print '(a)', '['//s//']'
s=merge(mask=.true.,fsource=b,tsource=a)
print '(a)', '['//s//']'
print '(a)',merge('abc','def',.false.)
print '(a)',merge('abc','def',.true.)
end
''','[no   ]\n[yes  ]\ndef\nabc\n')

    def test_default_and_zero_length(self):
        self.check('''implicit none
character::a
character(0)::empty
a='a'
print '(a)',merge(a,'b',.true.)
empty=merge('','',.false.)
print *,len(empty)
end
''','a\n0\n')

    def test_scalar_broadcast_and_logical_array_masks(self):
        self.check('''implicit none
logical::mask(4)
character(3)::s(4)
mask=[.true.,.false.,.true.,.false.]
s=merge('yes','no ',mask)
print '(4(a,1x))',s
s=merge('yes','no ',.not.mask)
print '(4(a,1x))',s
end
''','yes no  yes no  \nno  yes no  yes \n')

    def test_arrays_scalar_masks_and_overlap(self):
        self.check('''implicit none
character(2)::a(4),b(4),s(4)
logical::mask(4)
a=['aa','bb','cc','dd'];b=['11','22','33','44']
mask=[.true.,.false.,.false.,.true.]
s=merge(a,b,mask)
print '(4a2)',s
s=merge(a,'!!',.false.)
print '(4a2)',s
a=merge(a(4:1:-1),a,mask)
print '(4a2)',a
end
''','aa2233dd\n!!!!!!!!\nddbbccaa\n')

    def test_numeric_expression_mask_and_nested_merge(self):
        self.check('''implicit none
real::x(4)
character(5)::s(4)
x=[0.0,0.25,0.75,1.0]
s=merge(merge('high ','low  ',x>0.5),'none ',x>0.0)
print '(4a5)',s
end
''','none low  high high \n')

    def test_rank_two_and_same_size_wrong_shapes(self):
        self.check('''implicit none
character(2)::a(2,2),b(2,2)
logical::mask(2,2)
a(1,1)='aa';a(2,1)='bb';a(1,2)='cc';a(2,2)='dd'
mask(1,1)=.true.;mask(2,1)=.false.;mask(1,2)=.false.;mask(2,2)=.true.
b=merge(a,'!!',.not.mask)
print '(4a2)',b
print *,shape(merge(a,'!!',mask))
end
''','!!bbcc!!\n2 2\n')
        self.reject('''implicit none
character(2)::a(2,2),b(4),s(2,2)
logical::mask(2,2)
a='aa';b='bb';mask=.true.
s=merge(a,b,mask)
end
''','shapes do not conform')

    def test_empty_arrays(self):
        self.check('''implicit none
character(3)::a(0),b(0)
logical::mask(0)
b=merge(a,'abc',mask)
print *,size(b),len(b)
print '(a)',merge('yes','no ',mask)
end
''','0 3\n\n')

    def test_pure_elemental_procedure(self):
        self.check('''implicit none
character(3)::s(2)
s=label([.true.,.false.])
print '(2a3)',s
contains
pure elemental function label(mask) result(s)
logical,intent(in)::mask
character(3)::s
s=merge('yes','no ',mask)
end function
end
''','yesno \n')

    def test_scalar_mask_evaluated_once_when_broadcast(self):
        self.check('''implicit none
integer::calls=0
character::s(3)
s=merge('a','b',choose())
print '(3a1)',s
print *,calls
contains
logical function choose()
calls=calls+1
choose=.true.
end function
end
''','aaa\n1\n')

    def test_user_procedure_named_merge(self):
        self.check('''implicit none
character(2)::s
s=merge('aa','bb',.true.)
print '(a)',s
contains
function merge(a,b,mask) result(s)
character(2),intent(in)::a,b
logical,intent(in)::mask
character(2)::s
s='ok'
end function
end
''','ok\n')

    def test_invalid_sources_and_mask(self):
        for expression,message in [("merge('a','bb',.true.)",'same type, kind and CHARACTER length'),
                                   ("merge('a',1,.true.)",'same type, kind and CHARACTER length'),
                                   ("merge(1,'a',.true.)",'same type, kind and CHARACTER length'),
                                   ("merge('a','b',1)",'MASK must be LOGICAL')]:
            with self.subTest(expression=expression):
                self.reject('implicit none\ncharacter(3)::s\ns='+expression+'\nend\n',message)


if __name__=='__main__':
    unittest.main()
