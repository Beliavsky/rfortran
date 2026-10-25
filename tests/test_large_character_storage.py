"""Large main-program CHARACTER storage must not exhaust the native stack."""
from pathlib import Path
import unittest
import test_deallocate as helpers

class LargeCharacterStorage(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_large_variable_and_expression_buffers(self):
        self.check((Path(__file__).parent / "programs/xlarge_character_storage.f90").read_text(),
                   "1200000 abab\n1200000 1200000\n", data="ab\n")

    def test_many_large_character_locals(self):
        declarations = "\n".join(f"character(len=16384)::s{i}" for i in range(80))
        statements = "\n".join(f's{i}=repeat(seed,8192)\nn=n+iachar(s{i}(1:1))' for i in range(80))
        self.check(f"implicit none\n{declarations}\ninteger::n\ncharacter(len=2)::seed\nread (*,'(a)') seed\nn=0\n{statements}\nprint *,n\nend\n",
                   "5200\n", data="AB\n")

    def test_recursive_character_locals_remain_per_invocation(self):
        self.check("""print *,visit(3)
contains
recursive integer function visit(depth) result(n)
integer,intent(in)::depth
character(len=16384)::text
character::seed
seed=achar(65+depth)
text=repeat(seed,16384)
if(depth==0)then
 n=iachar(text(1:1))
else
 n=visit(depth-1)+iachar(text(1:1))
end if
end function
end
""", "266\n")

if __name__ == "__main__":
    unittest.main()
