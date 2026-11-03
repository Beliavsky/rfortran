"""Output symbol tables must isolate shadowing and generated temporaries."""
import unittest
import test_deallocate


class OutputScopeSharing(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_shadowed_character_arrays_and_sibling_scopes(self):
        self.check('''implicit none
character(len=2) :: a(2)
integer :: i
a=['ab','cd']
block
character(len=3) :: a(1)
a=['xyz']
print '(a)',(a(i),i=1,size(a))
end block
print '(a)',(a(i),i=1,size(a))
block
integer :: a(2)
a=[7,8]
print *, (a(i),i=1,size(a))
end block
print *, (a(i),i=1,size(a))
end
''', 'xyz\nab\ncd\n7 8\nabcd\n')

    def test_runtime_unit_and_formatted_sibling(self):
        self.check('''implicit none
integer :: unit,i,k
character(len=2) :: a(2)
a=['ab','cd']
read *,unit
do k=1,2
if(k==1) then
write(unit,*) (a(i),i=1,size(a))
else
write(unit,'(2a)') (a(i),i=1,size(a))
end if
end do
print '(2a)',(a(i),i=1,size(a))
end
''', 'abcd\nabcd\nabcd\n', data='6\n')

    def test_runtime_character_views(self):
        self.check('''implicit none
character(len=2),allocatable :: a(:)
integer :: n,i,k
read *,n
allocate(a(n))
a='xy'
do k=1,2
if(k==1) then
print *,a
else
print '(a)',(a(i),i=1,n)
end if
end do
print *,a
end
''', 'xyxyxy\nxy\nxy\nxy\nxyxyxy\n', data='3\n')


if __name__ == '__main__':
    unittest.main()
