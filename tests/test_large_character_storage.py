"""Large main-program CHARACTER storage must not exhaust the native stack."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate as helpers
from test_record_read import COMPILER

# Main-program arrays of 1 MiB or more, including locals of expanded calls.
LARGE_ARRAYS = """module m
contains
subroutine fill(x, total)
real, intent(in) :: x(:)
real, intent(out) :: total
real :: work(140000)
integer :: i
do i = 1, size(work)
 work(i) = x(mod(i-1, size(x)) + 1)
end do
total = sum(work)
end subroutine
end module
program p
use m
real :: a(140000), t
integer :: big(140000), k
character(len=1100000) :: text
a = 1.0
big = 3
text = 'abc'
do k = 1, 2
 call fill(a, t)
 print *, nint(t)
end do
call fill(a(1:10), t)
print *, nint(t), sum(big), len_trim(text), text(1:3)
end program
"""

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

    def test_large_arrays_are_not_in_the_executable_image(self):
        # Hundreds of expanded calls with such locals once produced images
        # over 2 GB, which Windows refuses to load.
        self.check(LARGE_ARRAYS, "140000\n140000\n140000 420000 3 abc\n")
        with tempfile.TemporaryDirectory(prefix="large-arrays-") as tmp:
            source = Path(tmp) / "large.f90"
            source.write_text(LARGE_ARRAYS, encoding="utf-8")
            exe = Path(tmp) / "large.exe"
            subprocess.run([COMPILER, "--quiet", source, "-o", exe], cwd=tmp, check=True, timeout=120)
            self.assertLess(exe.stat().st_size, 4_000_000)

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
