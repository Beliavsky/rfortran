"""CHARACTER array PARAMETERs and implied extents in interpreter/native O0/O2."""
from pathlib import Path
import unittest
import test_deallocate
from test_record_read import ROOT


class CharacterArrayParameters(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT / "tests/145_character_array_parameters.f90").read_text(encoding="utf-8"),
                   " aa bb cc\n aa bb cc\n")

    def test_lengths_references_bounds_and_implied_do(self):
        self.check("""program main
implicit none
character(len=3),parameter::p(*)=['a','b']
character(len=1),parameter::q(2)=['ab','cd']
character(len=*),parameter::r(*)=['abc','def']
character(len=3),parameter::copy(*)=r
character(len=3),parameter::joined(*)=[r,'ghi']
character(len=2),parameter::bounded(-1:1)=['aa','bb','cc']
character(len=2),parameter::first=bounded(-1)
integer::i
character(len=1),parameter::digits(*)=[(achar(48+i),i=1,3)]
character(len=0),parameter::empty(*)=[character(len=0)::]
integer,parameter::nums(*)=[1,2,3]
print '(2(a,1x))',p
print '(2(a,1x))',q
print '(2(a,1x))',r
print '(2(a,1x))',copy
print '(3(a,1x))',joined
print '(3(a,1x))',bounded
print '(a)',first
print '(3(a,1x))',digits
print *,size(empty),size(nums),sum(nums)
end program main
""", "a   b   \na c \nabc def \nabc def \nabc def ghi \naa bb cc \naa\n1 2 3 \n0 3 6\n")

    def test_runtime_index(self):
        self.check("""program main
character(len=2),parameter::s(*)=['aa','bb','cc']
integer::i
read(*,*)i
print '(a)',s(i)
end program main
""", "bb\n", data="2\n")

    def test_invalid_declarations_and_writes(self):
        cases = [
            ("character(2),parameter::s(2)=['aa','bb','cc']", "", "initializer shape mismatch"),
            ("character(2),parameter::s(*)=['aa','bb']", "s(1)='cc'", "cannot modify PARAMETER"),
            ("character(2),parameter::s(*)=['aa','bb']", "read(*,*)s", "cannot modify PARAMETER"),
            ("character(2),parameter::s(*)=['aa','bb']", "print *,s(3)", "subscript is out of bounds"),
            ("character(2),parameter::s(*)=['a','bb']", "", "constructor elements must have equal lengths"),
            ("character(2),parameter::s(*)=[1,2]", "", "character constant expression"),
            ("character(2)::s(*)", "", "requires a PARAMETER"),
        ]
        for declaration, statement, message in cases:
            with self.subTest(declaration=declaration, statement=statement):
                self.reject(f"program main\n{declaration}\n{statement}\nend program main\n", message)


if __name__ == "__main__":
    unittest.main()
