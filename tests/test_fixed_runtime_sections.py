"""Runtime section bounds on fixed arrays, including specialized procedures."""
import unittest
import test_deallocate
from test_record_read import ROOT


class FixedRuntimeSections(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_matrix_reshape_and_columns(self):
        self.check((ROOT/'tests/192_fixed_runtime_sections.f90').read_text(),
                   '1 2\n3 4\n1 2 2 1\n')

    def test_rank_one_bounds_stride_and_empty(self):
        self.check('''program main
integer::a(5),lo,hi,step
a=[1,2,3,4,5]
lo=2
hi=5
step=2
print *,a(lo:hi:step),size(a(lo:hi:step)),sum(a(lo:hi:step))
step=-2
print *,a(5:1:step)
lo=4
hi=2
step=1
print *,size(a(lo:hi:step)),sum(a(lo:hi:step))
end
''','2 4 2 6\n5 3 1\n0 0\n')

    def test_overlap_and_nondefault_bounds(self):
        self.check('''program main
integer::a(-2:2),b(0:1,3:5),i,j,k
a=[1,2,3,4,5]
i=-1
j=2
a(i:j)=a(i-1:j-1)
print *,a
b=reshape([1,2,3,4,5,6],[2,3])
j=4
k=5
b(:,j:k)=b(:,j-1:k-1)
print *,b
j=3
print *,b(:,j)
end
''','1 1 2 3 4\n1 2 1 2 3 4\n1 2\n')

    def test_rank_three_logical_and_character(self):
        self.check('''program main
logical::a(2,2,2)
character(3)::s(2,2)
integer::j,k
a=.false.
j=2
k=1
a(:,j,k)=.true.
print *,a(:,j,k),a(:,1,k)
s(1,1)='one'
s(2,1)='two'
s(1,2)='cat'
s(2,2)='dog'
s(:,j)=s(2:1:-1,k)
print '(4(a,1x))',s
print *,size(s(:,j)),len(s(:,j))
end
''','T T F F\none two two one\n2 3\n')

    def test_complex_and_derived(self):
        self.check('''program main
type::pair
integer::i
real(8)::x
end type pair
type(pair)::a(2,2)
complex(8)::z(2,2)
integer::j
a=pair(2,3.d0)
z=cmplx(3.d0,4.d0,kind=8)
j=2
z(:,j)=conjg(z(:,j))
a(:,j)=a(2:1:-1,1)
print *,a(:,j)%i,a(:,j)%x
print *,real(z(:,j)),aimag(z(:,j))
end
''','2 2 3 3\n3 3 -4 -4\n')

    def test_copy_in_out_and_readonly_section_actual(self):
        self.check('''program main
integer::a(2,2),j
a=reshape([1,2,3,4],[2,2])
j=2
call bump(a(:,j))
call show(a(:,j))
print *,a
contains
subroutine bump(x)
integer,intent(inout)::x(:)
x=x+10
end subroutine
subroutine show(x)
integer,intent(in)::x(:)
print *,x
end subroutine
end
''','13 14\n1 2 13 14\n')

    def test_function_bound_is_evaluated_once(self):
        self.check('''module m
integer::calls=0
contains
integer function bound()
calls=calls+1
bound=2
end function
end module
program main
use m
integer::a(4)
a=[1,2,3,4]
print *,a(bound():4)
print *,calls
end
''','2 3 4\n1\n')

    def test_forall_overlap_preserves_all_iterations(self):
        self.check('''program main
integer::a(2,3),col,rows
a=reshape([1,2,3,4,5,6],[2,3])
rows=2
forall(col=1:3) a(:,col)=a(:rows,4-col)
print *,a
end
''','5 6 3 4 1 2\n')

    def test_rank_fifteen_scalar_selector(self):
        dims=','.join(['2']+['1']*13+['2'])
        selection=','.join([':']*14+['j'])
        self.check(f'''program main
integer::a({dims}),j
a=7
j=2
print *,size(a({selection})),sum(a({selection}))
end
''','2 14\n')

    def test_nested_function_matrix_slice_and_result_section(self):
        self.check('''program main
real(8)::a(2,2,2)
a=reshape([1.d0,2.d0,3.d0,4.d0,5.d0,6.d0,7.d0,8.d0],[2,2,2])
print *,columns(a)
contains
function columns(a) result(v)
real(8),intent(in)::a(:,:,:)
real(8)::v(size(a,1)*size(a,3))
integer::i,j,n
n=size(a,1)
do i=1,size(a,3)
j=1+(i-1)*n
v(j:j+n-1)=first_column(a(:,:,i))
end do
end function
function first_column(a) result(v)
real(8),intent(in)::a(:,:)
real(8)::v(size(a,1))
v=a(:,1)
end function
end
''','1 2 5 6\n')

    def test_zero_stride_and_bounds_are_checked(self):
        self.check('''integer::a(4),step
a=1
step=0
print *,a(::step)
end
''',error='stride must not be zero',reference=False)
        self.check('''integer::a(2,2),j
a=1
j=3
print *,a(:,j)
end
''',error='outside',reference=False)

    def test_pointer_association_keeps_target_identity(self):
        self.check('''program main
real(8),target::a(2,2)
real(8),pointer::p(:)
integer::j
a=reshape([1.d0,2.d0,3.d0,4.d0],[2,2])
j=2
p=>a(:,j)
print *,associated(p),size(p),sum(p)
j=1
p(1)=9.d0
print *,p,a(:,2)
print *,p(2:1:-1)
p(2:1:-1)=p(1:2)
print *,a(:,2)
nullify(p)
print *,associated(p)
end
''','T 2 7\n9 4 9 4\n4 9\n4 9\nF\n')


if __name__=='__main__':
    unittest.main(verbosity=2)
