"""Shape/type scope isolation and implicit input during array lowering."""
import unittest
import test_deallocate

class ArrayLoweringScopes(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_shapes_and_types_restored_after_blocks(self):
        self.check('''implicit none
integer :: a(2,3),i
a=reshape([(i,i=1,6)],[2,3])
block
real :: a(3,2)
a=2.0
print *,sum(a,dim=1)
end block
block
integer :: a
a=9
print *,a
end block
print *,a(2,3),sum(a,dim=2)
end
''', '6 6\n9\n6 9 12\n')

    def test_nested_implicit_read_destinations(self):
        self.check('''integer :: k
k=1
do while(k==1)
if(k>0) then
select case(k)
case(1)
read *,i,x
end select
end if
k=2
end do
print *,i,x
end
''', '7 2.5\n',data='7 2.5\n')

if __name__ == '__main__':
    unittest.main()
