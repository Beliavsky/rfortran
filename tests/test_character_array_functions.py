"""Bounded CHARACTER array results, read-only arrays, and FORALL snapshots."""
import unittest
from unittest.mock import patch
import test_deallocate
from test_record_read import ROOT

MUNGE="""function munge(xx,yy,delim) result(xy)
character(len=*),intent(in)::xx(:),yy(:),delim
character(len=len(xx)+len(yy)+len(delim))::xy(size(xx)*size(yy))
integer::ix,iy,nx
nx=size(xx)
forall(ix=1:size(xx),iy=1:size(yy))xy(ix+(iy-1)*nx)=trim(xx(ix))//delim//trim(yy(iy))
end function munge
"""

class CharacterArrayFunctions(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT/'tests/148_character_array_functions.f90').read_text(encoding='utf-8'),
                   "'a-1' 'b-1' 'a-2' 'b-2'\n")

    def test_variables_sections_unequal_sizes_and_lengths(self):
        self.check("""program main
character(2)::left(3),right(2)
character(5)::out(6)
left=['a ','b ','c ']
right=['x ','y ']
out=munge(yy=right,delim=':',xx=left)
print '(*(1x,a))','>'//out//'<'
print *,size(munge(left,right,':')),len(munge(left,right,':'))
print '(*(1x,a))',munge(left(2:3),right(1:1),':')
print '(*(1x,a))',munge(left(3:1:-1),right(1:1),':')
contains
"""+MUNGE+"end program main\n",'>a:x  < >b:x  < >c:x  < >a:y  < >b:y  < >c:y  <\n6 5\nb:x   c:x  \nc:x   b:x   a:x  \n')

    def test_reverse_section_omitted_bounds(self):
        # The installed Windows gfortran reports SIZE(left(::-1)) == 0;
        # explicit bounds are compared with gfortran in the preceding test.
        with patch('test_deallocate.shutil.which', return_value=None):
            self.check("""program main
character(1)::left(3)
left=['a','b','c']
print '(*(1x,a))',munge(left(::-1),['x'],':')
contains
"""+MUNGE+"end program main\n",'c:x b:x a:x\n')

    def test_nested_array_result_as_actual(self):
        self.check("""program main
print '(*(1x,a))',munge(munge(['a','b'],['1'],'-'),['X','Y'],':')
contains
"""+MUNGE+"end program main\n",'a-1:X b-1:X a-1:Y b-1:Y\n')

    def test_empty_arrays_and_zero_element_lengths(self):
        self.check("""program main
print *,size(munge([character(len=1)::],['x'],'-'))
print *,size(munge(['a'],[character(len=1)::],'-'))
print '(*(1x,a))',"'"//munge(['',''],['',''],'')//"'"
contains
"""+MUNGE+"end program main\n","0\n0\n'' '' '' ''\n")

    def test_scalar_extent_and_early_return(self):
        self.check("""program main
print '(*(1x,a))',fill(3)
contains
pure function fill(n) result(out)
integer,intent(in)::n
character(2)::out(n)
out='ok'
return
out='no'
end function fill
end program main
""",'ok ok ok\n')

    def test_character_forall_snapshot_and_index_scope(self):
        self.check("""program main
character(2)::a(3)
integer::i
i=99
a=['ab','cd','ef']
forall(i=1:3)a(i)=a(4-i)
print '(*(1x,a))',a
print *,i
end program main
""",'ef cd ab\n99\n')

    def test_type_rank_shape_and_duplicate_diagnostics(self):
        for declaration,actual,message in [
            ('character(1),intent(in)::a(2)',"['a','b','c']",'matching extent'),
            ('character(2),intent(in)::a(:)',"['a','b']",'matching element length'),
            ('character(len=*),intent(in)::a(:)',"[1,2]",'fixed length'),
            ('character(len=*),intent(in)::a(:)',"'a'",'array actual'),
        ]:
            with self.subTest(declaration=declaration,actual=actual):
                self.reject(f"program main\nprint *,copy({actual})\ncontains\nfunction copy(a) result(out)\n{declaration}\ncharacter(1)::out(2)\nout='x'\nend function copy\nend program main\n",message)
        self.reject("""program main
character(1)::a(2)
integer::i
a=['a','b']
forall(i=1:2)a(1)=a(i)
end program main
""",'same destination element')

    def test_uninitialized_input(self):
        self.reject("""program main
character(1)::a(2)
print *,copy(a)
contains
function copy(a) result(out)
character(len=*),intent(in)::a(:)
character(len=len(a))::out(size(a))
out=a
end function copy
end program main
""",'uninitialized variable')


if __name__=='__main__':
    unittest.main()
