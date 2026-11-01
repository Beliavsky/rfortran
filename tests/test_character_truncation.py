"""-Wcharacter-truncation, in -Wall as in gfortran: a CHARACTER value of
compile-time length assigned or initialized into a shorter fixed-length
destination. Lengths the compiler cannot know, deferred-length destinations
and values that fit are not reported.

Run after cargo build --bin rfort: python tests/test_character_truncation.py
"""
import re
import unittest
import test_unused_warnings as shared

SOURCE = '''implicit none
character (len=3) :: s, arr(2)
character(5) :: t
character :: c
character(len=*), parameter :: p = "abcdef"
character(len=:), allocatable :: d
integer, parameter :: n = 4
character(len=n) :: u
character(2) :: v = "xyz"
type rec
  character(2) :: code
end type
type(rec) :: r
t = "hello"
s = "abcde"
s = t
s = t // "x"
s = t(2:3)
s = t(1:5)
c = "ab"
u = p
d = "long text"
arr(1) = "abcd"
arr = ["abcd", "efgh"]
r%code = "abc"
s = trim(t)
s = "abc"
s(1:2) = "xyz"
s(2:) = "ab"
print "(a)", s, arr, c, u, d, v, r%code
end
'''


def reported(text):
    """(line, from, to) of each truncation warning."""
    found = re.findall(r'length (\d+) is truncated to length (\d+).*\n.*:(\d+):\d+', text)
    return [(int(line), int(a), int(b)) for a, b, line in found]


class CharacterTruncation(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_lines_match_gfortran(self):
        # The same lines gfortran -Wall reports, plus the substring targets.
        expected = [(9, 3, 2), (15, 5, 3), (16, 5, 3), (17, 6, 3), (19, 5, 3), (20, 2, 1),
                    (21, 6, 4), (23, 4, 3), (24, 4, 3), (25, 3, 2), (28, 3, 2)]
        for flags in (['-Wall'], ['-Wcharacter-truncation']):
            with self.subTest(flags=flags):
                warnings = self.check(SOURCE, flags)
                self.assertEqual(reported(warnings), expected, warnings)

    def test_controls(self):
        self.assertNotIn('[character-truncation]', self.check(SOURCE))
        self.assertNotIn('[character-truncation]', self.check(SOURCE, ['-Wall', '-Wno-character-truncation']))
        self.assertIn('error: CHARACTER value of length 5', self.check(SOURCE, ['-Werror=character-truncation'], errors=True))
        self.assertNotIn('[character-truncation]', self.check('!#allow(character-truncation)\n' + SOURCE, ['-Wall']))

    def test_procedures_and_modules(self):
        warnings = self.check('''module m
  implicit none
  character(len=4), parameter :: tag = "abcdefg"
contains
  subroutine label(name, out)
    character(len=*), intent(in) :: name
    character(len=2), intent(out) :: out
    out = name
    out = "toolong"
  end subroutine
end module m
''', ['-Wall'])
        self.assertEqual([(a, b) for _, a, b in reported(warnings)], [(7, 4), (7, 2)], warnings)


if __name__ == '__main__':
    unittest.main()
