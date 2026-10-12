"""Delayed constructor implied-DOs and checked runtime whole-array assignments."""
import unittest
import test_deallocate
from test_record_read import ROOT


class RuntimeConstructors(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_character_comparison_repro(self):
        self.check((ROOT / 'tests/147_runtime_constructors.f90').read_text(encoding='utf-8'),
                   'F T F\n1\n')

    def test_runtime_count_mixed_items_and_index_scope(self):
        self.check("""program main
integer::i,n,a(4)
i=99
read(*,*)n
a=[(2*i,i=1,n)]
print *,a,i
a=[-1,(i,i=1,n-2),9]
print *,a,i
end program main
""", '2 4 6 8 99\n-1 1 2 9 99\n',data='4\n')

    def test_descending_and_zero_trip_loops(self):
        self.check("""program main
integer::i,n,a(2),empty(0)
i=77
read(*,*)n
a=[(i,i=n,1,-2)]
print *,a,i
n=0
empty=[(i,i=1,n)]
print *,size(empty),i
end program main
""",'4 2 77\n0 77\n',data='4\n')

    def test_nested_dependent_bounds(self):
        self.check("""program main
integer::i,j,n,a(6)
i=77
j=88
read(*,*)n
a=[((10*i+j,j=1,i),i=1,n)]
print *,a,i,j
end program main
""",'11 21 22 31 32 33 77 88\n',data='3\n')

    def test_overlapping_assignment_and_declared_index_kind(self):
        self.check("""program main
integer::n,a(3)
integer(kind=8)::i
logical::tf(3)
i=17
a=[1,2,3]
tf=[.true.,.false.,.true.]
read(*,*)n
a=[(a(i),i=n,1,-1)]
print *,a,i
tf=[(.not.tf(i),i=1,n)]
print *,tf
a=[(kind(i),i=1,n)]
print *,a
end program main
""",'3 2 1 17\nF T F\n8 8 8\n',data='3\n')

    def test_empty_character_constructor_actual(self):
        self.check("""program main
call compare(['aa','bb'],[character(len=2)::])
contains
subroutine compare(xx,yy)
character(len=*),intent(in)::xx(:),yy(:)
logical::mask(size(xx))
integer::i
mask=[(all(yy/=xx(i)),i=1,size(xx))]
print '(*(1x,l1))',mask
end subroutine compare
end program main
""",'T T\n')

    def test_runtime_constructor_in_scalar_subroutine(self):
        self.check("""program main
integer::n
read(*,*)n
call show(n)
contains
subroutine show(n)
integer,intent(in)::n
integer::i,a(4)
a=[(i,i=1,n)]
print *,a
end subroutine show
end program main
""",'1 2 3 4\n',data='4\n')

    def test_runtime_shape_errors(self):
        program="""program main
integer::i,n,a(3)
read(*,*)n
a=[(i,i=1,n)]
print *,a
end program main
"""
        for count in (0,2,4,1000000):
            with self.subTest(count=count):
                self.check(program,error='constructor shape does not conform',data=f'{count}\n')

    def test_rejects_runtime_array_valued_items(self):
        self.reject("""program main
integer::i,n,a(2),b(2)
b=[1,2]
read(*,*)n
a=[(b,i=1,n)]
end program main
""",'runtime constructor items must currently be scalar')

    def test_invalid_constant_contexts_and_stride(self):
        self.reject("""program main
integer::i,n
integer,parameter::a(3)=[(i,i=1,n)]
end program main
""",'PARAMETER initializer')
        for step,message in [('0','stride must not be zero'),('n','stride must currently be a constant INTEGER')]:
            with self.subTest(step=step):
                self.reject(f"program main\ninteger::i,n,a(3)\nread(*,*)n\na=[(i,i=1,n,{step})]\nend program main\n",message)


if __name__ == '__main__':
    unittest.main()
