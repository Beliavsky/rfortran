"""Assignment to scalar derived components selected across record arrays."""
import unittest
import test_deallocate
from test_record_read import ROOT

HEADER="""program test
implicit none
type::date
integer::year
end type
type::stamp
type(date)::day
integer::hour=5
end type
type(stamp)::a(3)
type(date)::b(2)
a%day=date(0)
b(1)=date(2024)
b(2)=date(2025)
"""

class ComponentArrayAssignment(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_component_array_assignment.f90').read_text(encoding='utf-8'),
                   '2024 2025\n')

    def test_sections_vectors_overlap_and_siblings(self):
        self.check(HEADER+"""a([3,1])%day=b
print *,a%day%year,a%hour
a(2:)%day=a(:2)%day
print *,a%day%year,a%hour
a(2)%day=date(7)
print *,a%day%year
a%day=date(9)
print *,a%day%year,a%hour
end
""", '2025 0 2024 5 5 5\n2025 2025 0 5 5 5\n2025 7 0\n9 9 9 5 5 5\n')

    def test_rank_and_nested_paths(self):
        for rank in (2,3,15):
            with self.subTest(rank=rank):
                shape=','.join(['2']+['1']*(rank-1))
                index=','.join(['2']+['1']*(rank-1))
                section=','.join([':']+['1']*(rank-1))
                self.check(f"""program test
implicit none
type::leaf
integer::x
end type
type::middle
type(leaf)::child
end type
type::outer
type(middle)::child
integer::guard=5
end type
type(outer)::a({shape})
type(leaf)::d({shape})
d=leaf(7)
a%child%child=d
a({index})%child%child=leaf(9)
print *,sum(a%child%child%x),sum(a%guard)
a({section})%child%child=[leaf(11),leaf(12)]
print *,sum(a%child%child%x),sum(a%guard)
end
""", '16 10\n23 10\n')

    def test_character_logical_and_fixed_array_components(self):
        self.check("""program test
implicit none
type::date
integer::year
character(len=3)::label
logical::ok
real::x(2)
end type
type::stamp
type(date)::day
integer::hour=5
end type
type(stamp)::a(2)
type(date)::d(2)
d(1)=date(2024,'abc',.true.,[1.0,2.0])
d(2)=date(2025,'def',.false.,[3.0,4.0])
a%day=d
print *,a%day%year,a%day%label,a%day%ok
print *,a(1)%day%x,a(2)%day%x,a%hour
end
""", '2024 2025 abcdef T F\n1 2 3 4 5 5\n')

    def test_runtime_dummy_and_function_result(self):
        self.check("""module m
implicit none
type::date
integer::year
end type
type::time
integer::hour
end type
type::stamp
type(date)::day
type(time)::clock
end type
contains
function combine(d,t) result(dt)
type(date),intent(in)::d(:)
type(time),intent(in)::t(:)
type(stamp)::dt(size(d))
dt%day=d
dt%clock=t
end function
end module
use m
implicit none
type(date)::d(2)
type(time)::t(2)
type(stamp)::a(2)
d=[date(2024),date(2025)]
t=[time(7),time(8)]
a=combine(d,t)
print *,a%day%year,a%clock%hour
end
""", '2024 2025 7 8\n')

    def test_component_of_scalar_container_array(self):
        self.check("""program test
implicit none
type::date
integer::year
end type
type::stamp
type(date)::day
end type
type::box
type(stamp)::items(3)
end type
type(box)::a
type(date)::d(2)
d=[date(2024),date(2025)]
a%items%day=date(0)
a%items(2:)%day=d
print *,a%items%day%year
end
""", '0 2024 2025\n')

    def test_runtime_allocated_parent(self):
        self.check("""program test
implicit none
type::date
integer::year
end type
type::stamp
type(date)::day
integer::hour=5
end type
type(stamp),allocatable::a(:)
type(date),allocatable::d(:)
integer::n
read *,n
allocate(a(n),d(n))
d=date(7)
a%day=d
print *,size(a),sum(a%day%year),sum(a%hour)
end
""", '4 28 20\n',data='4\n')

    def test_pointer_parent(self):
        self.check(HEADER.replace('type(stamp)::a(3)', 'type(stamp),target::a(3)\ntype(stamp),pointer::p(:)')+"""p=>a
p(1:3:2)%day=b
print *,a%day%year,a%hour
end
""", '2024 0 2025 5 5 5\n')

    def test_intent_in_view_cannot_be_written(self):
        self.reject("""module m
implicit none
type::date
integer::year
end type
type::stamp
type(date)::day
end type
contains
subroutine bad(a)
type(stamp),intent(in)::a(:)
a%day=date(7)
end subroutine
end module
use m
implicit none
type(stamp)::a(2)
a%day=date(0)
call bad(a)
end
""", 'INTENT(IN)')

    def test_unset_sibling_is_not_initialized_by_view_assignment(self):
        self.reject("""program test
implicit none
type::date
integer::year
end type
type::stamp
type(date)::day
integer::hour
end type
type(stamp)::a(2)
a%day=date(7)
print *,a(1)%hour
end
""", 'uninitialized')

if __name__=='__main__':
    unittest.main()
