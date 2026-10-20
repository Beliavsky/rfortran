"""Caller-owned numeric/LOGICAL allocatable results and intrinsic assignment."""
import unittest
import test_deallocate
from test_record_read import ROOT


class NumericAllocatableResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_direct_assignment(self):
        self.check((ROOT/'tests/190_allocatable_function_results.f90').read_text(), '2 5\n')

    def test_automatic_result_with_scalar_argument(self):
        self.check((ROOT/'tests/405_automatic_array_result_assignment.f90').read_text(),
                   '10 20 30\n')

    def test_automatic_result_reallocation_and_empty(self):
        for kind, value, expected in [('integer', 'i', '1 2 3'),
                                      ('integer(8)', 'int(i,8)', '1 2 3'),
                                      ('real', 'real(i)', '1 2 3'),
                                      ('double precision', 'dble(i)', '1 2 3'),
                                      ('logical', 'i>1', 'F T T')]:
            with self.subTest(kind=kind):
                self.check(f'''program main
implicit none
{kind},allocatable::x(:)
integer::n
read(*,*)n
x=make_array(n)
print *,x
! Conforming assignment retains the existing lower bound.
deallocate(x)
allocate(x(-1:n-2))
x=make_array(n)
print *,lbound(x,1),ubound(x,1)
x=make_array(1)
print *,size(x),lbound(x,1),ubound(x,1)
x=make_array(0)
print *,allocated(x),size(x)
contains
function make_array(n) result(y)
integer,intent(in)::n
{kind}::y(n)
integer::i
do i=1,n
y(i)={value}
end do
end function
end
''', expected+'\n-1 1\n1 1 1\nT 0\n', data='3\n')

    def test_fixed_character_array_result(self):
        self.check('''program main
implicit none
character(3),allocatable::x(:)
x=words()
print '(2a)',x
contains
function words() result(y)
character(3)::y(2)
y=['abc','def']
end function
end
''', 'abcdef\n')

    def test_multidimensional_automatic_result(self):
        for rank in (2, 3):
            shape=','.join(['n']*rank)
            deferred=','.join([':']*rank)
            self.check(f'''module m
contains
function filled(n) result(y)
integer,intent(in)::n
integer::y({shape})
y=7
end function
end module
program main
use m,only:make_array=>filled
implicit none
integer,allocatable::x({deferred})
integer::n
read(*,*)n
x=make_array(n)
print *,shape(x),sum(x)
x=make_array(0)
print *,allocated(x),size(x)
end
''', ' '.join(['2']*rank+[str(7*2**rank)])+'\nT 0\n',data='2\n')

    def test_scalar_results_do_not_allocate(self):
        self.reject('''program main
implicit none
real,allocatable::x(:)
x=scalar(3)
contains
real function scalar(n)
integer,intent(in)::n
scalar=real(n)
end function
end
''', 'array `x` is used before ALLOCATE')
        self.reject('''program main
implicit none
real,allocatable::x(:)
real::a(2)
a=1.0
x=a(1)
end
''', 'array `x` is used before ALLOCATE')

    def test_automatic_result_self_reference(self):
        self.check('''program main
implicit none
integer,allocatable::x(:)
x=sequence(3)
x=append(x)
print *,x
contains
function sequence(n) result(y)
integer,intent(in)::n
integer::y(n),i
do i=1,n
y(i)=i
end do
end function
function append(a) result(y)
integer,intent(in)::a(:)
integer::y(size(a)+1)
y(:size(a))=a
y(size(y))=9
end function
end
''', '1 2 3 9\n')

    def test_output_helper(self):
        self.check('''module m
contains
pure subroutine set_alloc(a,b)
integer,intent(in)::a(:)
integer,allocatable,intent(out)::b(:)
allocate(b(size(a)))
b=a
end subroutine
pure function copied(a) result(b)
integer,intent(in)::a(:)
integer,allocatable::b(:)
call set_alloc(a,b)
end function
end module
program main
use m
print *,copied([2,5])
end
''', '2 5\n')

    def test_scalar_merge(self):
        self.check('''program main
logical::mask
mask=.true.
print *,merge(2,5,mask),merge(2,5,.not.mask)
print *,merge(2.5,5.5,mask),merge(2.5d0,5.5d0,.not.mask)
print *,merge(.false.,.true.,mask),merge(.false.,.true.,.not.mask)
print *,merge('yes','no ',mask),merge('yes','no ',.not.mask)
end
''', '2 5\n2.5 5.5\nF T\nyesno\n')

    def test_types_empty_arrays_and_repeated_calls(self):
        for kind,values,expected in [('integer','2,5','2 5'),
                                     ('integer(8)','2_8,5_8','2 5'),
                                     ('real','2.5,5.5','2.5 5.5'),
                                     ('double precision','2.5d0,5.5d0','2.5 5.5'),
                                     ('logical','.true.,.false.','T F')]:
            with self.subTest(kind=kind):
                self.check(f'''module m
contains
function copied(a) result(b)
{kind},intent(in)::a(:)
{kind},allocatable::b(:)
b=a
end function
end module
program main
use m
{kind}::a(2)
integer::i
a=[{values}]
do i=0,2
print *,size(copied(pack(a,[i>=1,i>=2])))
print *,copied(pack(a,[i>=1,i>=2]))
end do
end
''', '0\n\n1\n'+expected.split()[0]+'\n2\n'+expected+'\n')

    def test_runtime_input_and_forwarded_result(self):
        self.check('''program main
integer::n
integer,allocatable::a(:)
read(*,*)n
allocate(a(n))
a=7
call consume(copied(a))
contains
function copied(a) result(b)
integer,intent(in)::a(:)
integer,allocatable::b(:)
b=a+1
end function
subroutine consume(a)
integer,intent(in)::a(:)
print *,size(a),sum(a)
end subroutine
end
''','4 32\n',data='4\n')

    def test_reallocation_and_self_assignment_snapshot(self):
        self.check('''program main
print *,filtered([-3,0,2,5])
print *,size(filtered([-3,0]))
contains
function filtered(a) result(b)
integer,intent(in)::a(:)
integer,allocatable::b(:)
b=a
b=pack(b,b>0)
b=2*b
b=b
end function
end
''','4 10\n0\n')

    def test_rank_two_and_three(self):
        for rank in (2,3):
            dims=','.join(['2']*rank)
            deferred=','.join([':']*rank)
            count=2**rank
            self.check(f'''program main
integer::a({dims})
a=reshape([{','.join(str(i) for i in range(1,count+1))}],[{dims}])
print *,shape(copied(a)),sum(copied(a))
contains
pure function copied(a) result(b)
integer,intent(in)::a({deferred})
integer,allocatable::b({deferred})
b=a
end function
end
''', ' '.join(['2']*rank+[str(count*(count+1)//2)])+'\n')

    def test_bounds_are_retained_when_shape_conforms(self):
        self.check('''program main
print *,bounded([2,5])
contains
function bounded(a) result(b)
integer,intent(in)::a(:)
integer,allocatable::b(:)
allocate(b(-1:0))
b=a
if(lbound(b,1)/=-1) stop 1
if(ubound(b,1)/=0) stop 2
end function
end
''','2 5\n')


    def test_rank_fifteen(self):
        dims=','.join(['2']+['1']*14)
        deferred=','.join([':']*15)
        self.check(f"""program main
integer::a({dims})
a=7
print *,size(copied(a)),sum(copied(a))
contains
function copied(a) result(b)
integer,intent(in)::a({deferred})
integer,allocatable::b({deferred})
b=a
end function
end
""",'2 14\n')

    def test_runtime_matrix(self):
        self.check("""program main
integer::n
integer,allocatable::a(:,:)
read(*,*)n
allocate(a(n,2))
a=3
print *,shape(copied(a)),sum(copied(a))
contains
function copied(a) result(b)
integer,intent(in)::a(:,:)
integer,allocatable::b(:,:)
b=a
end function
end
""",'4 2 24\n',data='4\n')

    def test_result_in_while_condition(self):
        self.check("""program main
integer::n
n=3
do while(size(copied(n))>0)
print *,n
n=n-1
end do
contains
function copied(n) result(b)
integer,intent(in)::n
integer,allocatable::b(:)
allocate(b(n))
b=1
end function
end
""",'3\n2\n1\n')

    def test_pos_keep_helper_path(self):
        self.check("""module m
contains
pure subroutine set_alloc(ii,jj)
integer,intent(in)::ii(:)
integer,allocatable,intent(out)::jj(:)
allocate(jj(size(ii)))
jj=ii
end subroutine
pure function pos_keep(words,exclude) result(ipos)
character(*),intent(in)::words(:),exclude(:)
integer,allocatable::ipos(:)
integer::i,jpos(size(words))
forall(i=1:size(words)) jpos(i)=merge(0,i,any(exclude==words(i)))
call set_alloc(pack(jpos,jpos>0),ipos)
end function
end module
program main
use m
print *,pos_keep(['a','c','d'],['a'])
print *,size(pos_keep(['a','c','d'],['a','c','d']))
print *,pos_keep(['a','c','d'],['z'])
end
""",'2 3\n0\n1 2 3\n')

if __name__=='__main__':
    unittest.main(verbosity=2)

