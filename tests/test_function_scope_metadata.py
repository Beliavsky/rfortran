"""Local bindings must not inherit descriptors belonging to another scope."""
import unittest

import test_deallocate
from test_record_read import ROOT


class FunctionScopeMetadata(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_xacv_reproducer(self):
        self.check((ROOT / 'tests/programs/xacv_function_dummy_descriptor_shadow.f90').read_text(), '7\n')

    def test_dummy_rank_shadows_main_descriptor(self):
        for rank in (1, 3, 15):
            with self.subTest(rank=rank):
                axes = ','.join([':'] * rank)
                shape = ','.join(['1'] * rank)
                self.check(f'''module scope_rank_mod
implicit none
contains
integer function cell(xx) result(n)
integer,intent(in)::xx({axes})
n=xx({shape})
end function
end module
use scope_rank_mod
implicit none
integer,allocatable::xx(:,:),input({axes})
allocate(xx(2,2),input({shape}))
input=9
print *,cell(input)
end
''', '9\n')

    def test_scalar_shadows_main_array(self):
        self.check('''module scope_scalar_mod
implicit none
contains
integer function twice(xx) result(n)
integer,intent(in)::xx
n=xx*2
end function
end module
use scope_scalar_mod
implicit none
integer,allocatable::xx(:,:)
allocate(xx(2,2))
print *,twice(4)
end
''', '8\n')

    def test_local_allocation_metadata_is_reloaded(self):
        self.check('''module scope_local_mod
implicit none
contains
integer function total(n) result(value)
integer,intent(in)::n
integer,allocatable::xx(:)
allocate(xx(-2:n-3))
xx=3
value=sum(xx)+lbound(xx,1)
end function
end module
use scope_local_mod
implicit none
integer,allocatable::xx(:,:)
allocate(xx(2,2))
print *,total(4),total(0)
end
''', '10 1\n')

    def test_block_preserves_inherited_array_descriptor(self):
        self.check('''implicit none
integer,allocatable::xx(:,:)
integer::n
read *,n
allocate(xx(-1:n-2,3:4))
xx=5
block
integer::answer
answer=sum(xx(:,3))
print *,answer,lbound(xx,1),size(xx,2)
end block
end
''', '15 -1 2\n', data='3\n')

    def test_character_dummy_uses_its_own_length(self):
        self.check('''module scope_character_mod
implicit none
contains
integer function last_code(xx) result(n)
character(len=*),intent(in)::xx
n=iachar(xx(len(xx):len(xx)))
end function
end module
use scope_character_mod
implicit none
character(len=:),allocatable::xx
xx='unrelated'
print *,last_code('abc'),last_code('z')
end
''', '99 122\n')


if __name__ == '__main__':
    unittest.main()
