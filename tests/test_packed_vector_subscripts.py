"""Runtime-sized index vectors passed through procedure arguments."""
from pathlib import Path
import unittest
import test_deallocate as helpers


class PackedVectorSubscripts(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_derived_result_components(self):
        source=(Path(__file__).parent/'programs/xpacked_vector_subscript.f90').read_text(encoding='utf-8')
        self.check(source,'one tri\n1 2 5 6\n')

    def test_builtin_types_empty_repeated_indices_and_assignment(self):
        for kind,values,expected,fmt in [
            ('integer','[10,20,30]','30 10 10','*'),
            ('real','[10.,20.,30.]','30 10 10','*'),
            ('logical','[.true.,.false.,.false.]','F T T','*'),
            ('character(3)',"['one','two','tri']",'tri one one',"'(3(a,1x))'"),
        ]:
            with self.subTest(kind=kind):
                self.check(f'''program main
implicit none
{kind}::a(3)
logical::mask(3)
a={values}
mask=.true.
call show(pack([3,1,1],mask))
mask=.false.
call show(pack([3,1,1],mask))
mask=[.true.,.true.,.false.]
call change(a,pack([3,1,1],mask))
print {fmt},a
contains
subroutine show(idx)
integer,intent(in)::idx(:)
print *,size(idx)
print {fmt},a(idx)
end subroutine
subroutine change(x,idx)
{kind},intent(inout)::x(:)
integer,intent(in)::idx(:)
x(idx)=x([1,3])
end subroutine
end program
''',f'3\n{expected}\n0\n\n'+{'integer':'30 20 10','real':'30 20 10','logical':'F F T','character(3)':'tri two one'}[kind]+'\n')

    def test_two_vectors(self):
        self.check('''program main
implicit none
integer::a(2,3)
logical::rows(2),cols(3)
a=reshape([1,2,3,4,5,6],[2,3])
rows=.true.
cols=[.true.,.false.,.true.]
call show(pack([2,1],rows),pack([3,2,1],cols))
contains
subroutine show(i,j)
integer,intent(in)::i(:),j(:)
print *,a(i,j)
end subroutine
end program
''','6 5 2 1\n')

    def test_derived_array(self):
        self.check('''program main
implicit none
type point
integer::n
character(3)::tag
end type
type(point)::a(3)
logical::mask(3)
a(1)=point(10,'one')
a(2)=point(20,'two')
a(3)=point(30,'tri')
mask=.true.
call show(a,pack([3,1,1],mask))
contains
subroutine show(x,idx)
type(point),intent(in)::x(:)
integer,intent(in)::idx(:)
type(point)::y(size(idx))
integer::i
y=x(idx)
do i=1,size(y)
print '(i0,1x,a)',y(i)%n,y(i)%tag
end do
end subroutine
end program
''','30 tri\n10 one\n10 one\n')

    def test_complex_array(self):
        self.check('''program main
implicit none
complex::a(3)
logical::mask(3)
a=[(1.,2.),(3.,4.),(5.,6.)]
mask=.true.
call show(a,pack([3,1,1],mask))
contains
subroutine show(x,idx)
complex,intent(in)::x(:)
integer,intent(in)::idx(:)
complex::y(3)
y=x(idx)
print *,real(y),aimag(y)
end subroutine
end program
''','5 1 1 6 2 2\n')

    def test_invalid_bounds_and_duplicate_definitions(self):
        template='''program main
implicit none
integer::a(3)
logical::mask(2)
a=[10,20,30]
mask=.true.
call change(a,pack(INDICES,mask))
contains
subroutine change(x,idx)
integer,intent(inout)::x(:)
integer,intent(in)::idx(:)
x(idx)=0
end subroutine
end program
'''
        for indices,message in [('[1,4]','outside bounds'),('[1,1]','selects an element more than once')]:
            with self.subTest(indices=indices):
                self.check(template.replace('INDICES',indices),error=message,reference=False)


if __name__ == '__main__':
    unittest.main()
