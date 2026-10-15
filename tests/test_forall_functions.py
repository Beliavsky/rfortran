"""PURE specialized functions in FORALL: snapshots, shapes, and purity."""
from pathlib import Path
import unittest
import test_deallocate

IDENTITY="""pure function identity(x) result(y)
integer,intent(in)::x(:)
integer::y(size(x))
y=x
end function identity
"""

class ForallFunctions(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_repro(self):
        self.check((Path(__file__).resolve().parent/'178_forall_array_functions.f90').read_text(encoding='utf-8'), '0 1 3 6\n0 4 9 15\n')

    def test_overlapping_column_swap(self):
        self.check("""program main
integer::a(2,2),j
a(:,1)=[1,2]
a(:,2)=[3,4]
j=99
forall(j=1:2) a(:,j)=identity(a(:,3-j))
print *,a,j
contains
"""+IDENTITY+'end program main\n','3 4 1 2 99\n')

    def test_assumed_shape_subroutine(self):
        self.check("""program main
integer::a(2,2)
a(:,1)=[1,2]
a(:,2)=[3,4]
call swap(a)
print *,a
contains
subroutine swap(x)
integer,intent(inout)::x(:,:)
integer::j
forall(j=1:size(x,2)) x(:,j)=identity(x(:,3-j))
end subroutine swap
"""+IDENTITY+'end program main\n','3 4 1 2\n')

    def test_multi_index_scalar_result(self):
        self.check("""program main
integer::a(2,2),i,j
a(:,1)=[1,2]
a(:,2)=[3,4]
forall(i=1:2,j=1:2) a(i,j)=total(a(:,3-j))+i
print *,a
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
end program main
""",'8 9 4 5\n')

    def test_character_elemental_snapshot(self):
        self.check("""program main
character(len=2)::a(3)
integer::i
a=['aa','bb','cc']
forall(i=1:3) a(i)=copy(a(4-i))
print *,a
contains
elemental function copy(x) result(y)
character(len=*),intent(in)::x
character(len=len(x))::y
y=x
end function copy
end program main
""",'ccbbaa\n')

    def test_logical_elemental_snapshot(self):
        self.check("""program main
logical::a(3)
integer::i
a=[.true.,.false.,.false.]
forall(i=1:3) a(i)=copy(a(4-i))
print *,a
contains
pure elemental function copy(x) result(y)
logical,intent(in)::x
logical::y
y=x
end function copy
end program main
""",'F F T\n')

    def test_zero_iterations_do_not_read_input(self):
        self.check("""program main
integer::a(2),b(2),i
forall(i=2:1) b(i)=total(a)
print *,42
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
end program main
""",'42\n')

    def test_empty_array_result_in_constructor(self):
        self.check("""program main
integer::a(1,2),j
forall(j=1:2) a(:,j)=[9,identity([integer::])]
print *,a
contains
"""+IDENTITY+'end program main\n','9 9\n')

    def test_conformance_and_duplicate_destinations(self):
        self.reject("""program main
integer::a(2,2),j
forall(j=1:2) a(:,j)=identity([1,2,3])
contains
"""+IDENTITY+'end program main\n','conform')
        self.reject("""program main
integer::a(2,2),j
forall(j=1:2) a(:,1)=identity([1,2])
contains
"""+IDENTITY+'end program main\n','same destination element')

    def test_runtime_bounds_do_not_use_heap_placeholder_shape(self):
        self.check("""program main
integer,allocatable::a(:)
integer::n,i
n=3
allocate(a(n))
a=1
forall(i=1:size(a)) a(i)=total(a)
print *,a
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
end program main
""",'3 3 3\n')

    def test_impure_function_is_rejected(self):
        self.reject("""program main
character(len=1)::a(2)
integer::i
a=['a','b']
forall(i=1:2) a(i)=copy(a(i))
contains
impure elemental function copy(x) result(y)
character(len=*),intent(in)::x
character(len=len(x))::y
y=x
end function copy
end program main
""",'FORALL cannot call impure')

if __name__=='__main__':unittest.main()
