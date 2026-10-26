"""Fixed array sections passed to specialized subroutines use bounded-size IR."""
from pathlib import Path
import unittest
import test_deallocate as helpers

SOURCE = (Path(__file__).parent/'programs/xlarge_section_argument.f90').read_text()

class LargeSectionArguments(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_sections(self):
        self.check(SOURCE,"column 2048 4196352\nreverse 1024 3148800\nempty 0 0\n",interpreter=False)

    def test_rank_three_logical(self):
        self.check("""program main
implicit none
logical :: a(2048,2,2)
a=.false.
a(:,2,1)=.true.
call inspect(a(:,2,1),'logical')
contains
subroutine inspect(x,label)
logical,intent(in)::x(:)
character(len=*),optional,intent(in)::label
if(present(label)) print *,count(x),size(x)
end subroutine
end
""","2048 2048\n",interpreter=False)

    def test_million_elements(self):
        self.check("\n".join(line for line in SOURCE.splitlines() if "reverse" not in line).replace('2048','1000000').replace('integer :: a','integer(kind=8) :: a').replace('integer,intent(in)::x','integer(kind=8),intent(in)::x'),
                   "column 1000000 1000001000000\nempty 0 0\n",interpreter=False)

if __name__=='__main__':
    unittest.main()
