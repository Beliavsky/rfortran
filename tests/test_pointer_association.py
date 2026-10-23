"""Fixed TARGET aliases, including escaped column associations, at O0/O2."""
import unittest

import test_deallocate
from test_record_read import ROOT


class PointerAssociation(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_column_output_dummy_fixture(self):
        self.check((ROOT/"tests/135_pointer_association.f90").read_text(encoding="utf-8"),
                   "T 3 15 1 3\n7 8 6\n7 8 6\n8 9 7\n1 2 3\n1 2 3\nF\n")

    def test_associated_section_preserves_declared_lower_bounds(self):
        self.check("""program main
implicit none
integer,target::a(-2:2)
integer,pointer::p(:)
a=[1,2,3,4,5]
p=>a(0:2)
print *,associated(p,a(0:2)),associated(p,a(-1:1)),size(p)
end
""", "T F 3\n")

    def test_scalar_alias_and_multiple_nullify(self):
        self.check("""program main
integer,target::x,y
integer,pointer::p,q
x=1;y=2
p=>x
q=>y
x=3
print *,p,q,associated(p)
p=7
q=>x
q=q+1
print *,x,y,p,q
nullify(p,q)
print *,associated(p),associated(q)
end
""","3 2 T\n8 2 8 8\nF F\n")

    def test_strided_sections_and_overlapping_assignment(self):
        self.check("""program main
integer,target::a(6)
integer,pointer::p(:),q(:)
a=[1,2,3,4,5,6]
p=>a(6:2:-2)
q=>a(1:3)
print *,p,p(2),p(2:3),size(p)
p=q
print *,a
q=>a(2:4)
q=a(1:3)
print *,a
end
""","6 4 2 4 4 2 3\n1 3 3 2 5 1\n1 1 3 3 5 1\n")

    def test_reverse_sections_with_omitted_bounds(self):
        self.check("""program main
integer,target::a(4)
integer,pointer::p(:),q(:)
a=[1,2,3,4]
q=>a(::-1)
print *,associated(q),size(q),sum(q)
p=>a(4:1:-1)
print *,p,p(::-1),p(:1:-1),p(4::-1),size(p(::-1))
p(1)=9
print *,a
end
""","T 0 0\n4 3 2 1 4 1 0\n1 2 3 9\n")

    def test_association_captures_subscript_without_reading_target(self):
        self.check("""program main
integer,target::a(2,3)
integer,pointer::p(:)
integer::column
column=2
p=>a(:,column)
column=3
print *,size(p),shape(p)
p(1)=7
p(2)=8
print *,a(:,2)
end
""","2 2\n7 8\n")

    def test_conditional_reassociation_and_loop_condition(self):
        self.check("""program main
integer,target::a(2),b(2)
integer,pointer::p(:)
integer::i
a=[1,2];b=[3,4]
do i=1,2
if (i==1) then
p=>a
else
p=>b
end if
p(1)=p(1)+10
end do
do while (p(1)>10)
p(1)=p(1)-10
cycle
end do
print *,a,b,p
p=>null()
print *,associated(p)
end
""","11 2 3 4 3 4\nF\n")

    def test_zero_extent(self):
        self.check("""program main
integer,target::a(0)
integer,pointer::p(:)
p=>a
print *,associated(p),size(p),sum(p),lbound(p),ubound(p)
nullify(p)
end
""","T 0 0 1 0\n")

    def test_block_shadowing(self):
        self.check("""program main
integer,target::a(2)
integer,pointer::p(:)
a=[1,2]
p=>a
block
integer::a(2)
a=[8,9]
print *,a
end block
block
integer,target::b(2)
integer,pointer::p(:)
b=[3,4]
p=>b
p=7
print *,b
end block
print *,p,a
end
""","8 9\n7 7\n1 2 1 2\n")

    def test_scalar_output_pointer_and_default_integer_kind(self):
        self.check("""program main
integer,target::x
integer,pointer::p
x=4
call attach(x,p)
p=9
print *,x,p
contains
subroutine attach(x,p)
integer,intent(in),target::x
integer(kind=4),pointer,intent(out)::p
p=>x
end subroutine
end
""","9 9\n")

    def test_logical_alias_and_nullify_identifier(self):
        self.check("""program main
logical,target::flag
logical,pointer::p
integer::nullify
flag=.false.
nullify=3
p=>flag
p=.true.
print *,flag,p,nullify
if (.true.) nullify(p)
print *,associated(p)
end
""","T T 3\nF\n")

    def test_associated_user_function(self):
        self.check("""program main
integer,target::x
integer,pointer::p
x=5
p=>x
print *,associated(p)
contains
integer function associated(i)
integer,intent(in)::i
associated=i+1
end function
end
""","6\n")

    def test_escaping_local_target_and_readonly_target_are_rejected(self):
        self.reject("""program main
integer,pointer::p(:)
call bad(p)
contains
subroutine bad(p)
integer,pointer,intent(out)::p(:)
integer,target::a(2)
p=>a
end subroutine
end
""","cannot outlive local TARGET")
        self.reject("""program main
integer,target::a(2)
integer,pointer::p(:)
a=1
call bad(a,p)
contains
subroutine bad(a,p)
integer,target,intent(in)::a(:)
integer,pointer,intent(out)::p(:)
p=>a
p=2
end subroutine
end
""","cannot modify INTENT(IN) TARGET")

    def test_intent_out_clears_association_without_undefining_target(self):
        self.check("""program main
integer,target::a(2)
integer,pointer::p(:)
a=[1,2]
p=>a
call detach(p)
print *,a
contains
subroutine detach(p)
integer,pointer,intent(out)::p(:)
nullify(p)
end subroutine
end
""","1 2\n")

    def test_dynamic_association_bounds(self):
        self.check("""program main
integer,target::a(2,3)
integer,pointer::p(:)
integer::column
column=4
p=>a(:,column)
end
""",error="subscript outside")

    def test_initialized_targets(self):
        self.check("""program main
integer,target::x=3,a(2)=[1,2]
integer,pointer::p,q(:)
p=>x
q=>a
p=4
q=q+1
print *,x,a,p,q
end
""","4 2 3 4 2 3\n")

    def test_pointer_results_and_pure_association_are_rejected(self):
        self.reject("""program main
print *,f()
contains
function f() result(p)
integer,pointer::p
integer,target::x
x=1
p=>x
end function
end
""","POINTER function results")
        self.reject("""program main
call f()
contains
pure subroutine f()
integer,target::x
integer,pointer::p
p=>x
end subroutine
end
""","pointer association in PURE")

    def test_unassociated_reads_and_bounds(self):
        for declaration,statement,message in [
            ("integer,pointer::p", "nullify(p)\nprint *,p", "unassociated pointer `p`"),
            ("integer,target::a(2)\ninteger,pointer::p(:)", "nullify(p)\nprint *,size(p)", "unassociated pointer `p`"),
            ("integer,target::a(2)\ninteger,pointer::p(:)\ninteger::i", "a=1\np=>a\ni=3\nprint *,p(i)", "pointer `p` subscript outside bounds"),
        ]:
            self.check(f"program main\n{declaration}\n{statement}\nend",error=message)

    def test_uninitialized_target_checks_are_preserved(self):
        self.check("""program main
integer,target::a(2)
integer,pointer::p(:)
p=>a
a(1)=7
print *,p(1)
print *,p(2)
end
""",error="uninitialized variable `a`")

    def test_invalid_targets_and_unsupported_shapes(self):
        for declarations,statement,message in [
            ("integer::x\ninteger,pointer::p", "p=>x", "requires TARGET"),
            ("real,target::x\ninteger,pointer::p", "p=>x", "type or kind mismatch"),
            ("integer,target::a(2)\ninteger,pointer::p", "p=>a", "rank mismatch"),
        ]:
            self.reject(f"program main\n{declarations}\n{statement}\nend",message)


if __name__=="__main__":
    unittest.main(verbosity=2)
