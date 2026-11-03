"""Constructor scope pruning preserves inquiries, shadowing and snapshots."""
import unittest
import test_deallocate


class ConstructorScopeSharing(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_shadowed_arrays_and_sibling_scopes(self):
        self.check('''implicit none
integer :: a(128),i
a=[(i,i=1,128)]
block
integer :: a(65)
a=[(2*i,i=1,size(a))]
print *,size(a),sum(a)
end block
block
integer :: a(66)
a=[(3*i,i=1,size(a))]
print *,size(a),sum(a)
end block
a=[(a(i),i=size(a),1,-1)]
print *,size(a),a(1),a(128),sum(a)
end
''', '65 4290\n66 6633\n128 128 1 8256\n')

    def test_size_without_constructor_and_nested_control_flow(self):
        self.check('''implicit none
integer :: a(2,3),i,n
a=1
n=0
do i=1,size(a,2)
if(size(a,1)==2) then
n=n+size(a)
end if
end do
block
integer :: a(4)
a=7
select case(size(a))
case(4)
n=n+size(a)
end select
end block
print *,n,size(a),size(a,2)
end
''', '22 6 3\n')


if __name__ == '__main__':
    unittest.main()
