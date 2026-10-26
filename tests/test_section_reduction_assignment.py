"""Assign runtime reduction results into sections after their axes are known."""
from pathlib import Path
import unittest
import test_deallocate as helpers
SOURCE=(Path(__file__).parent/'programs/xderived_section_sum_dim.f90').read_text()

class SectionReductionAssignment(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_sum(self):
        self.check(SOURCE,'10 30 60 90 120 1 3 6 9 12\n')

    def test_average_expression(self):
        self.check(SOURCE.replace('dim=1)','dim=1)/(j-i+1)'),
                   '10 15 20 30 40 1 1.5 2 3 4\n')

    def test_other_reductions(self):
        for intrinsic,expected in [('minval','10 10 10 20 30 1 1 1 2 3'),
                                   ('maxval','10 20 30 40 50 1 2 3 4 5'),
                                   ('product','10 200 6000 24000 60000 1 2 6 24 60')]:
            with self.subTest(intrinsic=intrinsic):
                self.check(SOURCE.replace('=sum(',f'={intrinsic}('),expected+'\n')

    def test_variable_dim_and_mask(self):
        source=SOURCE.replace('integer::i,j','integer::i,j,axis').replace('y=x','axis=1\ny=x')
        source=source.replace('dim=1)','dim=axis,mask=x%values(i:j,:)>2)')
        self.check(source,'10 30 60 90 120 0 0 3 7 12\n')

    def test_scalar_reduction_broadcast(self):
        self.check(SOURCE.replace(',dim=1',''),'11 33 66 99 132 11 33 66 99 132\n')

    def test_plain_allocatable(self):
        self.check("""program main
implicit none
integer,allocatable::a(:,:),b(:,:)
integer::j,axis
allocate(a(3,2),b(3,2))
a=reshape([1,2,3,4,5,6],[3,2])
b=0
axis=1
do j=1,3
b(j,:)=sum(a(1:j,:),dim=axis)
end do
print *,b
end
""",'1 3 6 4 9 15\n')

    def test_rank_three_logical(self):
        self.check("""program main
implicit none
logical,allocatable::a(:,:,:),b(:,:)
integer::axis
allocate(a(2,3,2),b(2,2))
a=.true.
a(1,2,1)=.false.
axis=2
b(2:1:-1,:)=all(a,dim=axis)
print *,b
end
""",'T F T T\n')

    def test_overlapping_input(self):
        self.check("""program main
implicit none
integer,allocatable::a(:,:)
allocate(a(2,2))
a=reshape([1,2,3,4],[2,2])
a(:,1)=sum(a,dim=2)
print *,a
end
""",'4 6 3 4\n')

if __name__=='__main__':
    unittest.main()
