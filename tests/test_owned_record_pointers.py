"""Pointers into record-owned allocations share data, masks and target identity."""
import unittest
import test_deallocate
from test_record_read import ROOT

HEADER = """module m
implicit none
type::row
integer::x
real::r
logical::ok
end type
type::vector
type(row),allocatable::xx(:)
end type
contains
subroutine show(v)
type(row),intent(in)::v(:)
print *,v%x,v%r,v%ok
end subroutine
subroutine link(v,p)
type(vector),target,intent(inout)::v(:)
type(row),pointer,intent(out)::p(:)
p=>v(1)%xx
end subroutine
end module
use m
implicit none
type(vector),target::v(2)
type(row),pointer::p(:),q(:)
allocate(v(1)%xx(-1:1),v(2)%xx(3))
v(1)%xx=[row(1,1.5,.true.),row(2,2.5,.false.),row(3,3.5,.true.)]
v(2)%xx=v(1)%xx
"""

NESTED_PROGRAM = """module nested
implicit none
type::day
integer::year
end type
type::clock
integer::hour
end type
type::stamp
type(day)::date
type(clock)::time
end type
type::bundle
type(stamp),allocatable::xx(:)
end type
contains
subroutine bind(a,p)
type(bundle),target,intent(inout)::a(:)
type(stamp),pointer,intent(out)::p(:)
p=>a(1)%xx
end subroutine
end module
use nested
implicit none
type(bundle),target::v(1)
type(stamp),pointer::p(:)
allocate(v(1)%xx(2))
call bind(v,p)
p(1)%date%year=2024
p(2)%date%year=2025
p(1)%time%hour=7
print *,p%date%year,p(1)%time%hour
end
"""


class OwnedRecordPointers(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_owned_component_pointer.f90').read_text(encoding='utf-8'), '2 2024 2025\n')

    def test_component_descriptor_groups_are_independent(self):
        self.check('''program test
implicit none
type row
integer::x
end type
type owner
type(row),allocatable::xx(:),xx_more(:)
integer::unset
end type
type(owner),target::v(1)
type(row),pointer::p(:),q(:)
allocate(v(1)%xx(-1:0))
p=>v(1)%xx
p%x=7
allocate(v(1)%xx_more(3))
q=>v(1)%xx_more
q%x=9
print *,size(p),sum(p%x),size(q),sum(q%x)
print *,lbound(p),ubound(p)
deallocate(v(1)%xx_more)
nullify(q)
print *,sum(p%x)
deallocate(v(1)%xx)
nullify(p)
allocate(v(1)%xx(0))
p=>v(1)%xx
print *,associated(p),size(p)
nullify(p)
end
''','2 14 3 27\n-1 0\n14\nT 0')

    def test_assignment_allocates_component_before_association(self):
        types=HEADER.split('use m\nimplicit none\n',1)[0]
        self.check(types+'''use m
implicit none
type(vector),target::v(1)
type(row),pointer::p(:)
v(1)%xx=[row(4,1.5,.true.),row(5,2.5,.false.)]
p=>v(1)%xx
p(2)%x=9
print *,p%x,p%r,p%ok
print *,size(v(1)%xx),associated(p,v(1)%xx)
end
''','4 9 1.5 2.5 T F\n2 T')

    def test_data_masks_identity_and_bounds(self):
        self.check(HEADER+"""p=>v(1)%xx
q=>v(1)%xx
print *,associated(p,q),associated(p,v(1)%xx),associated(p,v(2)%xx)
print *,lbound(p),ubound(p),size(p)
p(-1)%x=9
q(0)%r=8.5
p(1)%ok=.false.
call show(v(1)%xx)
call show(v(2)%xx)
nullify(p,q)
print *,size(v(1)%xx)
end
""", 'T T F\n-1 1 3\n9 2 3 1.5 8.5 3.5 T F F\n1 2 3 1.5 2.5 3.5 T F T\n3\n')

    def test_normal_assignment_preserves_existing_alias(self):
        self.check(HEADER+"""p=>v(1)%xx
v(1)%xx=[row(7,7.5,.false.),row(8,8.5,.true.),row(9,9.5,.false.)]
print *,p%x,p%r,p%ok
p(-1)%x=17
call show(v(1)%xx)
print *,associated(p,v(1)%xx)
end
""", '7 8 9 7.5 8.5 9.5 F T F\n17 8 9 7.5 8.5 9.5 F T F\nT\n')

    def test_pointer_argument_can_escape_to_caller(self):
        self.check(HEADER+"""call link(v,p)
p(0)%x=22
call show(v(1)%xx)
end
""", '1 22 3 1.5 2.5 3.5 T F T\n')

    def test_nested_record_target_through_dummy(self):
        self.check(NESTED_PROGRAM, '2024 2025 7\n')

    def test_scalar_container_dummy_matches_xacv_layout(self):
        program=NESTED_PROGRAM
        program=program.replace('contains\nsubroutine bind', 'type::container\ntype(bundle),allocatable::dd(:)\nend type\ncontains\nsubroutine bind')
        program=program.replace('type(bundle),target,intent(inout)::a(:)','type(container),target,intent(inout)::a')
        program=program.replace('p=>a(1)%xx','p=>a%dd(1)%xx')
        program=program.replace('type(bundle),target::v(1)','type(container),target::v')
        program=program.replace('allocate(v(1)%xx(2))','allocate(v%dd(1))\nallocate(v%dd(1)%xx(2))')
        self.check(program,'2024 2025 7\n')

    def test_general_rank_and_zero_size(self):
        for rank in (1,2,3,15):
            with self.subTest(rank=rank):
                shape=','.join(['2']+['1']*(rank-1))
                deferred=','.join([':']*rank)
                index=','.join(['2']+['1']*(rank-1))
                self.check(f"""program test
implicit none
type::row
integer::x
end type
type::vector
type(row),allocatable::xx({deferred})
end type
type(vector),target::v(1)
type(row),pointer::p({deferred})
allocate(v(1)%xx({shape}))
p=>v(1)%xx
p%x=7
p({index})%x=9
print *,size(p),sum(p%x)
end
""", '2 16\n')
        self.check(HEADER+"""deallocate(v(2)%xx)
allocate(v(2)%xx(0))
p=>v(2)%xx
print *,associated(p),size(p)
nullify(p)
end
""", 'T 0\n')

    def test_runtime_extent_and_strided_reassociation(self):
        self.check("""program test
implicit none
type::row
integer::x
end type
type::vector
type(row),allocatable::xx(:)
end type
type(vector),target::v(2)
type(row),pointer::p(:),q(:)
integer::n
read *,n
allocate(v(1)%xx(n),v(2)%xx(n+1))
p=>v(1)%xx
p%x=3
q=>v(2)%xx
q%x=7
print *,size(p),sum(p%x)
p=>q(2:n+1:2)
p%x=p%x+1
print *,size(p),size(q),sum(q%x)
nullify(q)
print *,sum(p%x)
end
""", '5 15\n3 6 45\n24\n',data='5\n')

    def test_unallocated_and_uninitialized_checks(self):
        self.check(HEADER+"""deallocate(v(1)%xx)
p=>v(1)%xx
end
""", error='pointer association requires an allocated record component',reference=False)
        self.check(HEADER+"""deallocate(v(1)%xx)
allocate(v(1)%xx(2))
p=>v(1)%xx
p(1)%x=9
print *,p(2)%x
end
""", error='uninitialized',reference=False)

    def test_local_target_cannot_escape(self):
        self.reject("""module m
implicit none
type::row
integer::x
end type
type::vector
type(row),allocatable::xx(:)
end type
contains
subroutine bad(p)
type(row),pointer,intent(out)::p(:)
type(vector),target::v(1)
allocate(v(1)%xx(2))
p=>v(1)%xx
end subroutine
end module
use m
implicit none
type(row),pointer::p(:)
call bad(p)
end
""", 'cannot outlive local owned TARGET')

    def test_parent_requires_target(self):
        self.reject(HEADER.replace('type(vector),target::v(2)','type(vector)::v(2)')+"""p=>v(1)%xx
end
""", 'requires TARGET attribute on its parent object')

if __name__=='__main__':
    unittest.main()
