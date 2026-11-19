"""Storage shared by separately compiled objects: COMMON blocks, with and
without BLOCK DATA in another file, and the host variables of internal
procedures of a module procedure. Each object defines the storage it uses;
the linker keeps one copy, with the values BLOCK DATA gives."""
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_record_read import COMPILER

BLOCK_DATA = """block data init
implicit none
integer :: n
real :: v(3)
character(len=5) :: tag
common /cfg/ n, v, tag
data n /3/, v /1.5, 2.5, 3.5/, tag /'hello'/
end block data init
"""
WORK = """subroutine bump()
implicit none
integer :: n
real :: v(3)
character(len=5) :: tag
common /cfg/ n, v, tag
real :: total
common total
n = n + 1
v = 2*v
tag = 'world'
total = sum(v)
end subroutine
"""
MAIN = """program main
implicit none
integer :: n
real :: v(3)
character(len=5) :: tag
common /cfg/ n, v, tag
real :: total
common total
external init
print *, n, v, tag
call bump()
print *, n, v, tag, total
end program
"""
EXPECTED = "3 1.5 2.5 3.5 hello\n4 3.0 5.0 7.0 world 15.0\n"


class SeparateCompilation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="separate-")
        self.d = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def rfort(self, *args):
        result = subprocess.run([str(COMPILER), *map(str, args)], cwd=self.d, capture_output=True,
                                text=True, encoding="utf-8", errors="replace", timeout=180)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def run_exe(self, name):
        result = subprocess.run([str(self.d / name)], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def write(self, name, text):
        (self.d / name).write_text(text, encoding="utf-8")

    def assertOutput(self, actual, expected):
        self.assertEqual(actual.split(), expected.split(), actual)

    def test_common_blocks_and_block_data(self):
        for name, text in (("bd.f90", BLOCK_DATA), ("work.f90", WORK), ("main.f90", MAIN)):
            self.write(name, text)
        self.rfort("-c", "bd.f90", "-o", "bd.obj")
        self.rfort("-c", "work.f90", "-o", "work.obj")
        # In every order, and with the main program compiled separately too.
        self.rfort("main.f90", "bd.obj", "work.obj", "-o", "first.exe")
        self.assertOutput(self.run_exe("first.exe"), EXPECTED)
        self.rfort("main.f90", "work.obj", "bd.obj", "-o", "last.exe")
        self.assertOutput(self.run_exe("last.exe"), EXPECTED)
        self.rfort("-c", "main.f90", "-o", "main.obj")
        self.rfort("link", "work.obj", "main.obj", "bd.obj", "-o", "objects.exe")
        self.assertOutput(self.run_exe("objects.exe"), EXPECTED)

    def test_common_block_without_block_data(self):
        self.write("set.f90", "subroutine setc()\ncommon /blk/ x, k\nreal :: x\ninteger :: k\nx = 4.0\nk = 7\nend subroutine\n")
        self.write("main.f90", "program p\ncommon /blk/ x, k\nreal :: x\ninteger :: k\ncall setc()\nprint *, x, k\nend program\n")
        self.rfort("-c", "set.f90", "-o", "set.obj")
        self.rfort("main.f90", "set.obj", "-o", "main.exe")
        self.assertOutput(self.run_exe("main.exe"), "4.0 7")

    def test_host_variables_of_a_module_procedure(self):
        # The internal procedure's host variable is kept in module storage,
        # which belongs to the module's object.
        self.write("hs.f90", """module hs_mod
implicit none
contains
subroutine add(x)
real, intent(in) :: x
real, allocatable :: h(:)
allocate (h(0))
h = [h, x]
call show()
contains
subroutine show()
print *, h
end subroutine
end subroutine
end module
""")
        self.write("main.f90", "program p\nuse hs_mod\ncall add(1.0); call add(2.0)\nend program\n")
        self.rfort("-c", "hs.f90", "-o", "hs.obj")
        self.rfort("main.f90", "hs.obj", "-o", "main.exe")
        self.assertOutput(self.run_exe("main.exe"), "1.0 2.0")


if __name__ == "__main__":
    unittest.main(verbosity=2)
