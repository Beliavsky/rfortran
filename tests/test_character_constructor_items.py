"""CHARACTER array constructors with variables, intrinsic results and arrays
as items, assigned to fixed-size and ALLOCATABLE arrays and passed as
actual arguments, in the interpreter, native O0/O2 and gfortran."""
import unittest
import test_deallocate


class CharacterConstructorItems(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fixed_size_targets(self):
        self.check('''program p
implicit none
character(len=6) :: c(2), s, d(4)
s = 'ab'
c = [s, s]
print *, c
c = [repeat('x', 6), s]
print *, c
c = [adjustr(s), s(1:4)//'zz']
print *, c
d = [c, c(2), 'qq    ']
print *, d
call show([s, adjustr(s)])
contains
subroutine show(v)
character(len=*), intent(in) :: v(:)
print *, size(v), v
end subroutine
end program
''', ' ab    ab    \n xxxxxxab    \n     abab  zz\n     abab  zzab  zzqq    \n 2 ab        ab\n')

    def test_appending_to_allocatable_arrays(self):
        # An item that is the target itself, with intrinsic results.
        self.check('''program p
implicit none
integer, parameter :: nc = 6
character(len=nc), allocatable :: tags(:)
character(len=nc) :: s
s = 'ab'
allocate (tags(0))
tags = [tags, repeat('x', nc)]
tags = [tags, adjustl(s)]
tags = [tags, adjustr(s)]
print *, size(tags)
print '(3("[",a,"]"))', tags
end program
''', '3\n[xxxxxx][ab    ][    ab]\n')

    def test_typed_constructors_allocate_the_target(self):
        self.check('''program p
implicit none
character(len=6), allocatable :: tags(:)
character(len=6) :: s
s = 'ab'
tags = [character(len=6) :: s, 'cd']
print *, size(tags), tags
tags = [character(len=6) :: repeat('x', 3)]
print *, size(tags), tags
tags = [character(len=4) :: s, 'abcdefgh']
print *, size(tags), tags
end program
''', '2 ab    cd\n1 xxx\n2 ab    abcd\n')


if __name__ == '__main__':
    unittest.main(verbosity=2)
