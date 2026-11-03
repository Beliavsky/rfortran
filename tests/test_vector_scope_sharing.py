"""Shared vector-lowering environments must preserve lexical scope isolation."""
import unittest
import test_deallocate


class VectorScopeSharing(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_shadowing_and_sibling_scopes(self):
        self.check('''implicit none
integer :: a(4), idx(2), n
a=[10,20,30,40]
idx=[4,1]
n=2
if (n==2) then
   print *,a(idx)
   block
      integer :: a(3), idx(1)
      a=[5,6,7]
      idx=[2]
      print *,a(idx)
   end block
   print *,a(idx)
else
   print *,0
end if
block
   integer :: idx
   idx=3
   print *,a(idx)
end block
print *,a(idx)
end
''', '40 10\n6\n40 10\n30\n40 10\n')

    def test_runtime_descriptors_and_generated_temporaries(self):
        self.check('''implicit none
integer,allocatable :: a(:,:), idx(:)
integer :: n,i
n=3
allocate(a(n,2),idx(2))
a=reshape([1,2,3,4,5,6],[n,2])
idx=[3,1]
do i=1,2
   if (i==1) then
      print *,a(idx,i)
   else
      print *,a(idx,i)+10
   end if
end do
block
   integer :: idx(1)
   idx=[2]
   print *,a(idx,2)
end block
print *,a(idx,1)
end
''', '3 1\n16 14\n5\n3 1\n')


if __name__ == '__main__':
    unittest.main()
