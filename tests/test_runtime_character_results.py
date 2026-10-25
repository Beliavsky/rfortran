"""Descriptor-backed CHARACTER array function results."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO=(Path(__file__).parent/'programs/xunion_character_result.f90').read_text()
FUNCTION=REPRO.split('contains\n',1)[1].rsplit('end program',1)[0]
class RuntimeCharacterResults(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_union(self):
        self.check(REPRO, "3\nred\nblue\ngreen\n")

    def test_empty_repeated_and_nondefault_bounds(self):
        self.check("""program main
implicit none
character(8),allocatable::a(:),b(:),c(:)
integer::i
allocate(a(-2:-1),b(4:5))
a=['one     ','two     ']
b=['two     ','three   ']
do i=1,3
 c=union_cols(a,b)
 print *,size(c),len(c)
 print '(a)',c
end do
deallocate(a,b)
allocate(a(0),b(0))
c=union_cols(a,b)
print *,size(c),len(c)
deallocate(b)
allocate(b(2))
b=['four    ','four    ']
c=union_cols(a,b)
print *,size(c)
print '(a)',c
contains
"""+FUNCTION+"end program\n", "3 8\none\ntwo\nthree\n"*3+"0 8\n1\nfour\n")

    def test_expression_output_and_forwarding(self):
        self.check("""program main
implicit none
character(8),allocatable::a(:),b(:)
allocate(a(1),b(1))
a='one'
b='two'
print *,size(union_cols(a,b))
print '(a)',union_cols(a,b)
call show(union_cols(a,b))
contains
subroutine show(x)
character(*),intent(in)::x(:)
print *,size(x),len(x)
print '(a)',x
end subroutine
"""+FUNCTION+"end program\n", "2\none\ntwo\n2 8\none\ntwo\n")

    def test_implicit_result_allocation_and_early_return(self):
        self.check("""implicit none
character(4),allocatable::a(:),c(:)
allocate(a(3))
a=['one ','two ','last']
c=copy(a,.true.)
print *,size(c)
print '(a)',c
c=copy(a,.false.)
print *,size(c)
contains
function copy(x,yes) result(y)
character(4),intent(in)::x(:)
logical,intent(in)::yes
character(4),allocatable::y(:)
if(yes)then
 y=x
 return
end if
allocate(y(0))
end function
end
""", "3\none\ntwo\nlast\n0\n")

    def test_fixed_destination_padding_and_matrix_rank(self):
        self.check("""implicit none
character(3),allocatable::a(:,:)
character(4)::fixed(2,2)
allocate(a(2,2))
a=reshape(['one','two','tri','end'],[2,2])
fixed=copy(a)
print *,shape(fixed),len(fixed)
print '(a)',fixed
contains
function copy(x) result(y)
character(3),intent(in)::x(:,:)
character(3),allocatable::y(:,:)
y=x
end function
end
""", "2 2 4\none\ntwo\ntri\nend\n")

    def test_result_exceeds_bounded_specialization_limit(self):
        self.check("""implicit none
character(2),allocatable::a(:),b(:)
integer::n
n=2049
allocate(a(n))
a='ok'
b=tail(a)
print *,size(b),all(b=='ok')
contains
function tail(x) result(y)
character(2),intent(in)::x(:)
character(2),allocatable::y(:)
allocate(y(size(x)-1))
y=x(2:)
end function
end
""", "2048 T\n", interpreter=False)  # Large character copies exceed the interpreter's test timeout.

    def test_fixed_destination_shape_error(self):
        self.check("""program main
implicit none
character(8),allocatable::a(:),b(:)
character(8)::fixed(1)
allocate(a(1),b(1))
a='one'
b='two'
fixed=union_cols(a,b)
contains
"""+FUNCTION+"end program\n", error="shapes do not conform")

if __name__=='__main__':unittest.main()
