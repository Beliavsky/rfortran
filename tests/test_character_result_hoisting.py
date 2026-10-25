"""Preserve allocation attributes when moving generated CHARACTER storage."""
from pathlib import Path
import unittest
import test_deallocate as helpers

SOURCE = (Path(__file__).parent / 'programs/xcharacter_result_loop.f90').read_text(encoding='utf-8')


class CharacterResultHoisting(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_loop_with_changing_and_empty_lengths(self):
        self.check(SOURCE, 'one\n\nthree\n')

    def test_conditional_result(self):
        self.check(SOURCE.replace("   print '(a)', trimmed(words(i))",
                                 "   if (i /= 2) then\n      print '(a)', trimmed(words(i))\n   end if"),
                   'one\nthree\n')

    def test_block_result(self):
        self.check(SOURCE.replace("   print '(a)', trimmed(words(i))",
                                 "   block\n      print '(a)', trimmed(words(i))\n   end block"),
                   'one\n\nthree\n')


if __name__ == '__main__':
    unittest.main()
