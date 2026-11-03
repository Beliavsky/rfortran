"""Deferred CHARACTER arrays must survive early constant-folding snapshots."""
import unittest
import test_deallocate
from test_record_read import ROOT


class EarlyDeferredCharacterArrays(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_optional_array_copy(self):
        self.check((ROOT / 'tests/programs/xoptional_character_array_copy.f90').read_text(),
                   'labels 2 3\none,two\nlabels 3 4\nfour,five,nine\n')

    def test_runtime_allocation_and_reallocation(self):
        modules = (ROOT / 'tests/programs/xoptional_character_array_copy.f90').read_text().split('program main')[0]
        self.check(modules + '''program main
use state_mod
implicit none
character(len=:), allocatable :: words(:)
integer :: n
call activate()
n = 4
allocate(character(len=n) :: words(2))
words = 'abcdef'
print *, len(words), size(words), words
deallocate(words)
words = [character(len=3) :: 'one', 'two', 'six']
print *, len(words), size(words), words
words = [character(len=n+2) ::]
print *, len(words), size(words), allocated(words)
words = [character(len=0) ::]
print *, len(words), size(words), allocated(words)
end program
''', '4 2 abcdabcd\n3 3 onetwosix\n6 0 T\n0 0 T\n')


if __name__ == '__main__':
    unittest.main(verbosity=2)
