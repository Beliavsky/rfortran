"""Large fixed sections use copy loops with overlap-safe RHS evaluation."""
from pathlib import Path
import unittest
import test_deallocate as helpers

class LargeSectionAssignments(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_million_row_matrix(self):
        self.check((Path(__file__).parent/'programs/xlarge_section_assignment.f90').read_text(),
                   '8000000 8 8\n',interpreter=False)

    def test_overlapping_shift_and_reverse(self):
        self.check("""program main
implicit none
integer :: a(4096),i
do i=1,4096
a(i)=i
end do
a(2:4096)=a(1:4095)
print *,a(1),a(2),a(4096)
a(4096:1:-1)=a(:)
print *,a(1),a(4095),a(4096),sum(a)
end
""",'1 1 4095\n4095 1 1 8386561\n',interpreter=False)

    def test_strided_logical(self):
        self.check("""program main
implicit none
logical :: a(4096)
a=.false.
a(1:4096:2)=.true.
a(2:4096:2)=.not.a(1:4096:2)
print *,count(a)
a(:)=.not.a(4096:1:-1)
print *,count(a),a(1),a(2)
end
""",'2048\n2048 T F\n',interpreter=False)

    def test_scalar_rhs_evaluated_once(self):
        self.check("""program main
implicit none
integer :: a(4096)
a=1
a(:)=sum(a)
print *,a(1),a(4096)
end
""",'4096 4096\n',interpreter=False)

if __name__=='__main__':
    unittest.main()
