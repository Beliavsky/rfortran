"""Large executable implied-DOs use loops without changing constructor semantics."""
import unittest
import test_deallocate


class LargeConstructors(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_large_nonconforming_constructor(self):
        self.check('''implicit none
integer :: i,a(3)
a=[(i,i=1,1000000)]
end
''', error='constructor shape does not conform', reference=False)

    def test_snapshot_stride_and_index_scope(self):
        self.check('''implicit none
integer :: a(128),i
i=777
a=[(i,i=1,128)]
a=[(a(i),i=128,1,-1)]
print *,a(1),a(128),sum(a),i
a=[(i,i=2,256,2)]
print *,a(1),a(128),sum(a),i
end
''', '128 1 8256 777\n2 256 16512 777\n')

    def test_nested_scope(self):
        self.check('''implicit none
integer :: a(128),i,k
do k=1,2
if (k>0) then
a=[(k*i,i=1,128)]
end if
end do
print *,sum(a)
end
''', '16512\n')

    def test_named_kind_and_allocatable_destination(self):
        self.check('''implicit none
integer,parameter :: dp=kind(1d0)
real(dp) :: a(128)
real(dp),allocatable :: b(:)
integer :: i
a=[(real(i,dp),i=1,128)]
b=[(real(i,dp),i=128,1,-1)]
print *,sum(a),size(b),b(1),b(128)
end
''', '8256 128 128 1\n')

    def test_complex_lowerer_keeps_expansion(self):
        self.check('''implicit none
complex :: z(128)
integer :: i
z=[(cmplx(i,-i),i=1,128)]
print *,real(z(128)),aimag(z(128))
end
''', '128 -128\n')

    def test_typed_constructors_keep_constant_conversion(self):
        self.check('''implicit none
real :: a(128)
character(len=3) :: b(128)
integer :: i
a=[real :: (i,i=1,128)]
b=[character(len=3) :: ('x',i=1,128)]
print *,sum(a),len(b),trim(b(128))
end
''', '8256 3 x\n')

    def test_nested_array_items_and_empty_loop(self):
        self.check('''implicit none
integer :: a(256),i,j
i=77
j=88
a=[((i+j,j=1,2),i=1,128),(i,i=1,0)]
print *,a(1),a(256),sum(a),i,j
a=[([i,-i],i=1,128)]
print *,a(1),a(256),sum(a),i
end
''', '2 130 16896 77 88\n1 -128 0 77\n')

    def test_character_logical_and_expression(self):
        self.check('''implicit none
integer :: i
character(len=2) :: a(128)
logical :: b(128)
i=9
a=[('xy',i=1,128)]
b=[(mod(i,2)==0,i=1,128)]
print *,a(128),count(b),sum(2*[(i,i=1,128)]),i
end
''', 'xy 64 16512 9\n')

    def test_constant_and_saved_initializers(self):
        self.check('''implicit none
integer :: i
integer,parameter :: a(128)=[(i,i=1,128)]
integer,save :: b(128)=[(2*i,i=1,128)]
integer :: c(a(128))
c=1
print *,sum(a),sum(b),size(c)
end
''', '8256 16512 128\n')


if __name__ == '__main__':
    unittest.main()
