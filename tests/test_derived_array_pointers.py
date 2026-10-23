"""Component-wise derived array pointer descriptors, native/interpreted/reference."""
import unittest
import test_deallocate
from test_record_read import ROOT

class DerivedArrayPointers(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_derived_array_pointer.f90').read_text(encoding='utf-8'),
                   '2 2024 2025\n2024 2030\n')

    def test_nested_fields_association_nullify_and_write_through(self):
        self.check("""program test
implicit none
type :: leaf
integer::n
real::x
logical::ok
end type
type :: row
type(leaf)::child
integer::tag
end type
type(row),target::a(2),b(2)
type(row),pointer::p(:),q(:)
integer,pointer::ip(:)
nullify(p,q,ip)
print *,associated(p),associated(q)
a(1)=row(leaf(1,2.0,.true.),10)
a(2)=row(leaf(3,4.0,.false.),20)
b=a
p=>a
q=>p
print *,associated(pointer=p,target=a),associated(p,b),associated(q,p)
p(2)%child%x=9
q(1)%tag=7
print *,a(2)%child%x,a(1)%tag
q=>null()
nullify(p)
print *,associated(p),associated(q),a(2)%child%x
end
""", 'F F\nT F T\n9 7\nF F 9\n')

    def test_sections_bounds_and_pointer_reassociation(self):
        self.check("""program test
implicit none
type :: row
integer::x,y
end type
type(row),target::a(-1:3)
type(row),pointer::p(:),q(:)
integer::i
do i=-1,3
a(i)=row(i,10*i)
end do
p=>a
print *,lbound(p),ubound(p),size(p)
q=>p(3:1:-1)
print *,q%x,q%y
p=>q
q=>a(1:2)
p(1)%y=99
print *,a(3)%y,associated(q,a(1:2))
end
""", '-1 3 5\n3 2 1 30 20 10\n99 T\n')

    def test_association_captures_parent_subscript(self):
        self.check("""program test
implicit none
type::row
integer::n
real::x
end type
type(row),target::a(2,2)
type(row),pointer::p(:)
integer::column
column=2
a=row(1,2.0)
p=>a(:,column)
column=1
p(1)%n=9
print *,column,a(1,2)%n,a(1,1)%n
end
""", '1 9 1\n')

    def test_general_rank(self):
        for rank in (2,3,15):
            with self.subTest(rank=rank):
                dims=','.join(['2']+['1']*(rank-1))
                deferred=','.join([':']*rank)
                idx=','.join(['2']+['1']*(rank-1))
                self.check(f"""program test
implicit none
type :: row
integer::n
real::x
end type
type(row),target::a({dims})
type(row),pointer::p({deferred})
a=row(2,3.0)
p=>a
p({idx})%n=7
p({idx})%x=9
print *,size(p),sum(a%n),sum(a%x)
end
""", '2 9 12\n')

    def test_array_component_of_scalar_target(self):
        self.check("""program test
implicit none
type::row
integer::x
end type
type::holder
type(row)::values(2)
end type
type(holder),target::box
type(row),pointer::p(:)
box%values(1)%x=3
box%values(2)%x=5
p=>box%values
p(1)%x=7
print *,box%values%x,associated(p,box%values)
end
""", '7 5 T\n')

    def test_pointer_dummy_escape(self):
        self.check("""module m
implicit none
type::row
integer::x
end type
contains
subroutine attach(a,p)
type(row),target,intent(inout)::a(:)
type(row),pointer,intent(out)::p(:)
p=>a(2:)
end subroutine
end module
use m
implicit none
type(row),target::a(3)
type(row),pointer::p(:)
a%x=[1,2,3]
call attach(a,p)
p(1)%x=8
print *,a%x,associated(p)
end
""", '1 8 3 T\n')

    def test_type_and_rank_mismatches(self):
        self.reject("""program test
implicit none
type::one
integer::x
end type
type::two
integer::x
end type
type(one),pointer::p(:)
type(two),target::a(2)
p=>a
end
""", 'derived array type mismatch')

    def test_rank_mismatch_and_missing_target(self):
        program = """program test
implicit none
type::row
integer::x
end type
type(row),pointer::p(:)
type(row),target::a(2,2)
p=>a
end
"""
        self.reject(program, 'rank mismatch in pointer association')
        self.reject(program.replace(',target::a(2,2)', '::a(2)'),
                    'requires TARGET attribute')

    def test_unsupported_component_layout_is_diagnosed(self):
        program = """program test
implicit none
type::row
character(len=2)::x
end type
type(row),pointer::p(:)
end
"""
        self.reject(program, 'derived pointer arrays currently require scalar numeric or LOGICAL components')
        self.reject(program.replace('character(len=2)::x','integer,allocatable::x(:)'),
                    'derived pointer arrays currently require scalar numeric or LOGICAL components')

    def test_uninitialized_component_is_not_read_at_association(self):
        self.check("""program test
implicit none
type::row
integer::x,y
end type
type(row),target::a(2)
type(row),pointer::p(:)
p=>a
p(1)%x=5
print *,a(1)%x
end
""", '5\n')

if __name__ == '__main__':
    unittest.main()
