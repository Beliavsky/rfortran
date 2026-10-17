"""Vector selection across types/ranks, with descriptor and overlap checks."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_character_comparisons
from test_record_read import ROOT, COMPILER
import test_vector_subscripts


class VectorRanks(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare
    check_input = test_vector_subscripts.VectorSubscripts.check_input

    def check(self, source, expected):
        with tempfile.TemporaryDirectory(prefix="vector-ranks-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text(source, encoding="utf-8")
            self.compare(path, expected, directory)

    def test_character_repro(self):
        with tempfile.TemporaryDirectory(prefix="vector-character-") as tmp:
            self.compare(ROOT / "tests/programs/xcharacter_vector_subscript.f90", "verb adj verb\n", Path(tmp))

    def test_every_rank(self):
        declarations, statements = [], ["idx=[2,1]"]
        for rank in range(1, 16):
            dims = ",".join(["2"] + ["1"] * (rank - 1))
            indices = ",".join(["idx"] + ["1"] * (rank - 1))
            element = ",".join(["1"] * rank)
            declarations.append(f"integer::a{rank}({dims})")
            statements += [f"a{rank}=20", f"a{rank}({element})=10",
                           f"print *,a{rank}({indices})",
                           f"a{rank}({indices})=[30,40]", f"print *,a{rank}({indices})"]
        self.check("program main\nimplicit none\ninteger::idx(2)\n" +
                   "\n".join(declarations + statements) + "\nend\n",
                   "20 10\n30 40\n" * 15)

    def test_intrinsic_types_and_character_padding(self):
        for kind, values, expected in [
            ("integer(1)", "[1_1,2_1,3_1,4_1]", "4 3 2 1"),
            ("integer(2)", "[1_2,2_2,3_2,4_2]", "4 3 2 1"),
            ("integer(8)", "[10000000001_8,2_8,3_8,4_8]", "4 3 2 10000000001"),
            ("real", "[1.5,2.5,3.5,4.5]", "4.5 3.5 2.5 1.5"),
            ("real(8)", "[1.5d0,2.5d0,3.5d0,4.5d0]", "4.5 3.5 2.5 1.5"),
            ("logical", "[.true.,.false.,.false.,.true.]", "T F F T"),
            ("character(len=3)", '["one","two","tri","for"]', "for tri two one"),
        ]:
            with self.subTest(kind=kind):
                self.check(f"""program main
implicit none
{kind}::a(2,2),b(2,2)
integer::idx(2)
idx=[2,1]
{('a(1,1)="one"; a(2,1)="two"; a(1,2)="tri"; a(2,2)="for"' if kind.startswith('character') else f'a=reshape({values},[2,2])')}
b=a(idx,idx)
{("print '(4(a,1x))',b" if kind.startswith('character') else 'print *,b')}
a(idx,idx)=a
{("print '(4(a,1x))',a" if kind.startswith('character') else 'print *,a')}
end
""", expected + "\n" + expected + "\n")
        self.check("""character(3)::a(3),b(2)
a=['abc','def','ghi']
b=a([3,1])
b([2,1])=['x','y']
print '(4a)', '[',b(1),b(2),']'
end
""", "[y  x  ]\n")
        self.check("""character(2)::a(3)
integer::idx(2)
a=['aa','bb','cc']
idx=[3,1]
print '(2(a,1x))',a(idx)//'!'
a([3,1])=a([1,3])//'x'
print '(3(a,1x))',a
end
""", "cc! aa!\ncc bb aa\n")

    def test_mixed_axes_explicit_bounds_and_inquiries(self):
        self.check("""program main
implicit none
integer::a(-1:0,3:5,2),i,j,k,idx(2)
idx=[0,-1]
do k=1,2
do j=3,5
do i=-1,0
a(i,j,k)=100*k+10*j+i
end do
end do
end do
print *,a(idx,5:3:-2,2)
print *,shape(a(idx,5:3:-2,2)),size(a(idx,5:3:-2,2))
print *,lbound(a(idx,5:3:-2,2)),ubound(a(idx,5:3:-2,2))
a(idx,5:3:-2,2)=a([-1,0],3:5:2,1)
print *,a(idx,5:3:-2,2)
end
""", "250 249 230 229\n2 2 4\n1 1 2 2\n129 130 149 150\n")

    def test_runtime_extents_empty_vectors_and_overlap(self):
        self.check("""program main
implicit none
integer,allocatable::a(:,:),idx(:),out(:,:)
integer::n
do n=0,3
allocate(a(2,3),idx(n),out(n,2))
a=reshape([1,2,3,4,5,6],[2,3])
idx=1
out=a(idx,2:3)
print *,shape(out),sum(out)
deallocate(a,idx,out)
end do
allocate(a(2,3),idx(2))
a=reshape([1,2,3,4,5,6],[2,3])
idx=[2,1]
a(idx,2:3)=a([1,2],1:2)
print *,a
end
""", "0 2 0\n1 2 8\n2 2 16\n3 2 24\n1 2 2 1 4 3\n")

    def test_character_runtime_section_of_index_vector(self):
        self.check("""program main
implicit none
character(3)::a(3),b(3)
integer::idx(3),n
a=['one','two','tri']
idx=[3,1,2]
do n=0,3
print '(3(a,1x))',a(idx(:n))
end do
end
""", "\ntri\ntri one\ntri one two\n")

    def test_readonly_procedure_actual(self):
        self.check("""program main
integer::a(2,2),idx(2)
a=reshape([1,2,3,4],[2,2])
idx=[2,1]
call show(a(idx,idx))
print *,selected_sum(a,idx)
contains
subroutine show(x)
integer,intent(in)::x(:,:)
print *,x
end subroutine show
pure integer function selected_sum(x,i)
integer,intent(in)::x(:,:),i(:)
selected_sum=sum(x(i,2))
end function selected_sum
end
""", "4 3 2 1\n7\n")

    def test_derived_components_and_overlap(self):
        self.check("""program main
implicit none
type :: point
integer :: n
real :: x
logical :: good
character(2) :: tag
end type point
type(point)::a(2,2),b(2,2)
integer::idx(-1:0),i,j
idx=[2,1]
do j=1,2
do i=1,2
a(i,j)=point(10*j+i,1.5,.true.,'xy')
end do
end do
b=a(idx,idx)
print *,b(1,1)%n,b(2,2)%n,b(1,1)%x,b(1,1)%good,b(1,1)%tag
print '(4(i0,1x,f3.1,1x,l1,1x,a,1x))',b(idx,idx)
a(idx,idx)=a
print *,a(1,1)%n,a(2,2)%n
end
""", "22 11 1.5 T xy\n11 1.5 T xy 12 1.5 T xy 21 1.5 T xy 22 1.5 T xy\n22 11\n")

    def test_runtime_bounds_and_duplicate_assignment(self):
        self.check_input("""program main
integer,allocatable::a(:,:),idx(:)
integer::j
allocate(a(2,2),idx(2))
a=7
read(*,*)j
idx=[1,j]
a(idx,2)=0
print *,a
end
""", [("2\n", "7 7 0 0", None),
       ("1\n", None, "more than once"),
       ("3\n", None, "outside bounds")])

    def test_writable_procedure_actual_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="vector-writable-") as tmp:
            directory = Path(tmp)
            source = directory / "main.f90"
            source.write_text("""integer::a(2),idx(2)
a=1
idx=[2,1]
call change(a(idx))
contains
subroutine change(x)
integer,intent(out)::x(:)
x=0
end subroutine change
end
""", encoding="utf-8")
            result = subprocess.run([COMPILER, "emit-obj", source, directory / "bad.obj"],
                                    capture_output=True, text=True, timeout=60)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("writable dummy", result.stderr)

    def test_large_vector_and_runtime_operand_evaluated_once(self):
        self.check("""program main
implicit none
integer,allocatable::idx(:),picked(:)
integer::a(2),n
a=[10,20]
n=1500
allocate(idx(n),picked(n))
idx=1
picked=a(idx+offset())
print *,size(picked),sum(picked)
contains
integer function offset()
print *, 'offset'
offset=1
end function offset
end
""", "offset\n1500 30000\n")

    def test_character_and_record_rank_fifteen(self):
        dims = ",".join(["2"] + ["1"] * 14)
        indices = ",".join(["idx"] + ["1"] * 14)
        first = ",".join(["1"] * 15)
        last = ",".join(["2"] + ["1"] * 14)
        self.check(f"""program main
implicit none
type :: item
integer :: n
end type item
character(2)::a({dims})
type(item)::b({dims}),c(2)
integer::idx(2)
idx=[2,1]
a='ab'
a({last})='xy'
print '(2(a,1x))',a({indices})
b({first})=item(10)
b({last})=item(20)
c=b({indices})
print *,c(1)%n,c(2)%n
end
""", "xy ab\n20 10\n")

    def test_runtime_rank_fifteen_result(self):
        deferred = ",".join([":"] * 15)
        allocation = ",".join(["n"] + ["1"] * 14)
        selection = ",".join(["idx"] + [":"] * 14)
        self.check(f"""program main
implicit none
integer,allocatable::a({deferred}),idx(:)
integer::n
n=2
allocate(a({allocation}),idx(n))
a=7
idx=[2,1]
print *,size(a({selection})),sum(a({selection}))
end
""", "2 14\n")

    def test_inquiries_do_not_read_array_or_index_values(self):
        self.check("""program main
implicit none
integer::missing(2,3),idx(2)
character(5)::words(3)
print *,shape(missing(idx,2)),size(missing(idx,2))
print *,lbound(missing(idx,2)),ubound(missing(idx,2))
print *,len(words(idx)),kind(words(idx))
end
""", "2 2\n1 2\n5 1\n")

    def test_matrix_section_as_index_vector(self):
        self.check("""program main
implicit none
integer::a(2,3)
a=reshape([1,2,2,1,3,1],[2,3])
print *,a(:,a(:,1))
end
""", "1 2 2 1\n")

    def test_runtime_derived_array_selection(self):
        self.check("""program main
implicit none
type :: item
integer :: n
logical :: good
end type item
type(item),allocatable::a(:,:),b(:,:)
integer,allocatable::idx(:)
integer::i,j
allocate(a(2,2),b(2,2),idx(2))
idx=[2,1]
do j=1,2
do i=1,2
a(i,j)=item(10*j+i,i==j)
end do
end do
b=a(idx,idx)
a(idx,idx)=a
print *,b(1,1)%n,b(2,2)%n,b(1,1)%good,b(1,2)%good
print *,a(1,1)%n,a(2,2)%n
end
""", "22 11 T F\n22 11\n")

    def test_typed_empty_index_constructor(self):
        self.check("""program main
implicit none
character(2)::words(3),empty(0)
integer,allocatable::a(:)
print *,size(words([integer::]))
empty=words([integer::])
allocate(a(3))
print *,size(a([integer::]))
a([integer::])=1
end
""", "0\n0\n")

    def test_pointer_source_and_pointer_index_vector(self):
        self.check("""program main
implicit none
integer,target::a(2,3),idx(2)
integer,pointer::p(:,:),q(:)
a=reshape([1,2,3,4,5,6],[2,3])
idx=[2,1]
p=>a(2:1:-1,3:1:-1)
q=>idx
print *,p(q,[3,1])
p(q,[3,1])=p([1,2],[1,3])
print *,a
end
""", "1 2 5 6\n6 5 3 4 2 1\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
