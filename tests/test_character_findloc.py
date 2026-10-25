"""CHARACTER FINDLOC through native and interpreted location lowering."""
from pathlib import Path
import unittest
import test_deallocate as helpers


class CharacterFindloc(unittest.TestCase):
    check=helpers.Deallocate.check
    reject=helpers.Deallocate.reject

    def test_allocatable_component(self):
        source=(Path(__file__).parent/'programs/xfindloc_character.f90').read_text(encoding='utf-8')
        self.check(source,'1\n3\n0\n0\n')

    def test_lengths_masks_and_sections(self):
        self.check('''program main
implicit none
character(3)::a(4)=['a  ','   ','abc','a  ']
print *,findloc(a,'a'),findloc(a,'a   ',back=.true.)
print *,findloc(a,'abcd',dim=1),findloc(a,'',dim=1)
print *,findloc(a,'a',dim=1,mask=.false.),findloc(a,'a',mask=[.false.,.true.,.true.,.true.])
print *,findloc(a(4:1:-1),'abc',dim=1)
print *,findloc(a(:0),'a'),findloc(a(:0),'a',dim=1)
end program
''','1 4\n0 2\n0 4\n2\n0 0\n')

    def test_rank_two_dim_and_kind(self):
        self.check('''program main
implicit none
character(2)::a(2,3)
a(:,1)=['aa','bb']
a(:,2)=['bb','aa']
a(:,3)=['aa','bb']
print *,findloc(a,'aa'),findloc(a,'aa',back=.true.)
print *,findloc(a,'aa',dim=1,kind=8)
print *,findloc(a,'bb',dim=2,back=.true.)
print *,findloc(a,'bb',dim=1,mask=.false.)
end program
''','1 1 1 3\n1 2 1\n2 3\n0 0 0\n')

    def test_runtime_extent_and_mask(self):
        self.check('''program main
implicit none
character(3),allocatable::a(:,:)
logical,allocatable::mask(:,:)
integer::n
logical::back
n=2
allocate(a(n,3),mask(n,3))
a(:,1)=['aa ','bb ']
a(:,2)=['bb ','aa ']
a(:,3)=['aa ','bb ']
mask=.true.
mask(1,1)=.false.
back=.true.
print *,findloc(a,'aa',mask=mask),findloc(a,'aa',back=back)
print *,findloc(a,'bb',dim=2,kind=8)
deallocate(a,mask)
allocate(a(0,3))
print *,findloc(a,'aa'),findloc(a,'aa',dim=1)
end program
''','2 2 1 3\n2 1\n0 0 0 0 0\n')

    def test_empty_strings_and_mismatched_value_type(self):
        self.check('''program main
implicit none
character(0)::a(2)
a=''
print *,findloc(a,'',dim=1),findloc(a,' ',back=.true.,dim=1)
print *,findloc(a,'x',dim=1)
end program
''','1 2\n0\n')
        self.reject("character(2)::a(0)\nprint *,findloc(a,1)\nend\n",'same type and kind')

    def test_keyword_order_and_value_expression(self):
        self.check('''program main
implicit none
character(3)::a(3)=['one','two','one']
character(8)::key='one'
logical::back
back=.true.
print *,findloc(back=back,value=trim(key),array=a,kind=8,dim=1,mask=.true.)
print *,findloc(a,key//'x',dim=1)
end program
''','3\n0\n')

    def test_character_sum_remains_rejected(self):
        self.reject("character(2)::a(2)\na='ab'\nprint *,sum(a)\nend\n",'requires numeric or LOGICAL arrays')


if __name__=='__main__':
    unittest.main()
