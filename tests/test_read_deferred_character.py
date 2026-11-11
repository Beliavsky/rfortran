"""List-directed READ into allocated deferred-length CHARACTER scalars."""
import unittest

import test_deallocate


class ReadDeferredCharacter(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_internal_read_keeps_allocated_length(self):
        self.check("""implicit none
character (len=:), allocatable :: x
character (len=10) :: s
s = "cat dog"
allocate(character(len=len(s)) :: x)
read (s,*) x
print *, "[", x, "]", len(x)
end
""", " [cat       ] 10\n")

    def test_constant_and_runtime_lengths_with_other_items(self):
        # A value longer than the variable is truncated; a shorter one is
        # padded. Neither changes the allocated length.
        self.check("""implicit none
character(len=:), allocatable :: x, y
character(len=20) :: s
integer :: n
s = "elephant 42 zebra"
allocate(character(len=4) :: x)
n = 6
allocate(character(len=n) :: y)
read (s,*) x, n, y
print *, "[", x, "]", len(x), n, "[", y, "]", len(y)
end
""", " [elep] 4 42 [zebra ] 6\n")


if __name__ == '__main__':
    unittest.main()
