"""Bounded runtime implied-DO actuals and allocatable output capacity."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE=(ROOT/'xset_alloc_implied_do.f90').read_text(encoding='utf-8').split('program xset_alloc_implied_do')[0]


class ConstructorArguments(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_reproducer(self):
        self.check((ROOT/'xset_alloc_implied_do.f90').read_text(encoding='utf-8'), '1 2 3\n')

    def test_mixed_items_stride_nested_loops_and_index_scope(self):
        self.check(MODULE+"""use set_alloc_mod
integer::i,j,n
integer,allocatable::v(:)
i=99
j=77
n=3
call set_alloc([-1,((i+j,j=1,i),i=1,n),9],v)
print *,v,i,j
deallocate(v)
n=5
call set_alloc([(i,i=n,1,-2)],v)
print *,v,i
deallocate(v)
end
""", '-1 2 3 4 4 5 6 9 99 77\n5 3 1 99\n')

    def test_empty_and_multiple_capacities(self):
        self.check(MODULE+"""use set_alloc_mod
integer::i,n
integer,allocatable::v(:)
n=0
call set_alloc([(i,i=1,n)],v)
print *,allocated(v),size(v)
deallocate(v)
n=4
call set_alloc([(i*2,i=1,n)],v)
print *,v
deallocate(v)
n=2
call set_alloc([(i,i=1,n)],v)
print *,v
end
""", 'T 0\n2 4 6 8\n1 2\n')

    def test_branch_local_bound(self):
        self.check(MODULE+"""use set_alloc_mod
integer::i,n,choice
integer,allocatable::v(:)
read(*,*)choice
if(choice==1) then
n=2
call set_alloc([(i,i=1,n)],v)
else
n=3
call set_alloc([(i,i=1,n)],v)
end if
print *,v
end
""", '1 2\n',data='1\n')

    def test_size_bound(self):
        self.check(MODULE+"""use set_alloc_mod
integer::i,n,a(3)
integer,allocatable::v(:)
n=size(a)
call set_alloc([(i,i=1,n)],v)
print *,v
end
""", '1 2 3\n')

    def test_other_numeric_kinds_and_logical_elements(self):
        for kind,expression,expected in [('integer(8)','7_8','7 7 7\n'),
                                         ('real','0.5','0.5 0.5 0.5\n'),
                                         ('double precision','0.25d0','0.25 0.25 0.25\n'),
                                         ('logical','i>1','F T T\n')]:
            with self.subTest(kind=kind):
                module=MODULE.replace('integer, intent(in) :: ii',kind+', intent(in) :: ii').replace('integer, allocatable, intent(out) :: jj',kind+', allocatable, intent(out) :: jj')
                self.check(module+f"""use set_alloc_mod
integer::i,n
{kind},allocatable::v(:)
n=3
call set_alloc([({expression},i=1,n)],v)
print *,v
end
""",expected)

    def test_unknown_or_invalidated_capacity(self):
        for statements in ['read(*,*)n', 'n=3\nread(*,*)n', 'n=3\ncall change(n)',
                           'n=3\nif(n>0) n=5']:
            with self.subTest(statements=statements):
                self.reject(MODULE+"""use set_alloc_mod
integer::i,n
integer,allocatable::v(:)
"""+statements+"""
call set_alloc([(i,i=1,n)],v)
contains
subroutine change(x)
integer::x
x=5
end subroutine
end
""",'runtime-bound array constructor arguments are not yet supported')

    def test_user_procedure_named_like_intrinsic_invalidates_bound(self):
        self.reject(MODULE+"""use set_alloc_mod
integer::i,n
integer,allocatable::v(:)
n=3
call set_alloc([(dble(i),i=1,n)],v)
contains
integer function dble(x) result(y)
integer,intent(in)::x
n=5
y=x
end function
end
""",'runtime-bound array constructor arguments are not yet supported')


if __name__=='__main__':
    unittest.main(verbosity=2)
