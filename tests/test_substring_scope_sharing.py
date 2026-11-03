"""Substring environments remain isolated across nested and sibling blocks."""
import unittest
import test_deallocate


class SubstringScopeSharing(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_shadowing_and_runtime_substring_snapshots(self):
        self.check('''implicit none
character(6) :: s
integer :: first,last
read *,first,last
s='abcdef'
block
character(:),allocatable :: s
s='12345678'
s(first:last)=s(first+1:last+1)
print '(a)',s
end block
block
integer :: s(3)
s=[4,5,6]
print *,sum(s)
end block
s(first:last)=s(first+1:last+1)
print '(a)',s
end
''', '13455678\n15\nacdeef\n',data='2 4\n')


if __name__ == '__main__':
    unittest.main()

