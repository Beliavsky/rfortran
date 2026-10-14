"""Function argument/result ranks independent of PURE, through rank fifteen."""
from pathlib import Path
import unittest
import test_deallocate

REPRO=(Path(__file__).parent/'181_multirank_array_function.f90').read_text(encoding='utf-8')

class MultirankArrayFunctions(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_above_diag_pure_and_ordinary(self):
        self.check(REPRO,'4.0 7.0 8.0\n')
        self.check(REPRO.replace('\nfunction above_diag','\npure function above_diag'),'4.0 7.0 8.0\n')

    def test_arguments_and_results_through_rank_fifteen(self):
        for rank in (2,3,4,15):
            for pure in ('','pure '):
                with self.subTest(rank=rank,pure=pure):
                    shape=','.join(['2']+['1']*(rank-2)+['2'])
                    assumed=','.join([':']*rank)
                    result=(', &\n '.join(','.join(f'size(x,{i})' for i in range(start,min(start+6,rank+1))) for start in range(1,rank+1,6)))
                    index=','.join(['2']+['1']*(rank-2)+['2'])
                    self.check(f'''program main
integer::a({shape}),b({shape})
a=reshape([3,7,11,19],[{shape}])
b=increment(a)
print *,sum(b),size(b),b({index})
contains
{pure}function increment(x) result(y)
integer,intent(in)::x({assumed})
integer::y({result})
y=x+1
end function increment
end program main
''','44 4 20\n')

    def test_different_result_rank_logical_and_forwarding(self):
        self.check('''program main
real::a(2,2,2),b(4,2)
logical::mask(2,2,2)
a=reshape([1.,2.,3.,4.,5.,6.,7.,8.],[2,2,2])
call outer(a,b)
mask=positive(a-4.)
print *,sum(b),count(mask),b(4,2)
contains
subroutine outer(x,y)
real,intent(in)::x(:,:,:)
real,intent(out)::y(:,:)
y=flatten_two(x)
end subroutine outer
pure function flatten_two(x) result(y)
real,intent(in)::x(:,:,:)
real::y(size(x,1)*size(x,2),size(x,3))
y=reshape(x,[size(x,1)*size(x,2),size(x,3)])
end function flatten_two
function positive(x) result(y)
real,intent(in)::x(:,:,:)
logical::y(size(x,1),size(x,2),size(x,3))
y=x>0.
end function positive
end program main
''','36 4 8\n')

    def test_ordinary_function_side_effects_once_per_call(self):
        self.check('''program main
integer::a(2,2),b(2,2),i
a=reshape([1,2,3,4],[2,2])
do i=1,2
 b=echo(a)
 print *,sum(b)
end do
contains
function echo(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
print *,'called'
y=x
end function echo
end program main
''','called\n10\ncalled\n10\n')

    def test_rank_two_sections(self):
        self.check('''program main
integer::a(3,2,2),b(2,2)
a=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[3,2,2])
b=copy(a(3:2:-1,2,:))
print *,b
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
y=x
end function copy
end program main
''','6 5 12 11\n')

    def test_character_rank_two_input_and_result(self):
        self.check('''program main
character(len=2)::a(2,2),b(2,2)
a(1,1)='a1';a(2,1)='b2';a(1,2)='c3';a(2,2)='d4'
b=echo(a)
print '(4(a,1x))',b
print *,size(echo(a),2)
contains
function echo(x) result(y)
character(len=*),intent(in)::x(:,:)
character(len=len(x))::y(size(x,1),size(x,2))
y=x
end function echo
end program main
''','a1 b2 c3 d4\n2\n')

    def test_character_rank_fifteen(self):
        shape=','.join(['2']+['1']*13+['2'])
        assumed=','.join([':']*15)
        result=', &\n '.join(','.join(f'size(x,{i})' for i in range(start,min(start+6,16))) for start in range(1,16,6))
        assignments='\n'.join(f"a({','.join([str(first)]+['1']*13+[str(last)])})='{text}'"
                              for first,last,text in [(1,1,'aa'),(2,1,'bb'),(1,2,'cc'),(2,2,'dd')])
        self.check(f'''program main
character(len=2)::a({shape}),b({shape})
{assignments}
b=copy(a)
print '(4(a,1x))',b
contains
function copy(x) result(y)
character(len=*),intent(in)::x({assumed})
character(len=len(x))::y({result})
y=x
end function copy
end program main
''','aa bb cc dd\n')

    def test_zero_extent_and_rank_mismatch(self):
        self.check('''program main
integer::a(2,0,3)
print *,size(copy(a))
contains
function copy(x) result(y)
integer,intent(in)::x(:,:,:)
integer::y(size(x,1),size(x,2),size(x,3))
y=x
end function copy
end program main
''','0\n')
        self.reject('''program main
integer::a(4)
a=1
print *,copy(a)
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
y=x
end function copy
end program main
''','rank mismatch')

    def test_array_expressions_reshape_and_transpose(self):
        self.check('''program main
integer::a(2,2),b(2,2)
a=reshape([1,2,3,4],[2,2])
b=copy(transpose(a))
print *,b
b=copy(reshape([5,6,7,8],[2,2]))
print *,b
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
y=x
end function copy
end program main
''','1 3 2 4\n5 6 7 8\n')

    def test_scalar_function_rank_two_section(self):
        self.check('''program main
integer::a(3,2,2),total
a=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[3,2,2])
total=add(a(3:2:-1,2,:))
print *,total
contains
function add(x) result(y)
integer,intent(in)::x(:,:)
integer::y
y=sum(x)
end function add
end program main
''','34\n')

    def test_scalar_result_with_multidimensional_expression(self):
        self.check('''program main
integer::a(2,2),total
a=reshape([1,2,3,4],[2,2])
total=add(a+1)
print *,total
total=add(transpose(a))
print *,total
contains
function add(x) result(y)
integer,intent(in)::x(:,:)
integer::y
y=sum(x)
end function add
end program main
''','14\n10\n')

    def test_unused_procedure_with_unresolved_pack_shape(self):
        self.check('''program main
print *,'ok'
contains
subroutine unused(x,mask,y)
real,intent(in)::x(:)
logical,intent(in)::mask(:)
real,intent(out)::y
y=average(pack(x,mask))
end subroutine unused
function average(x) result(y)
real,intent(in)::x(:)
real::y
y=sum(x)/size(x)
end function average
end program main
''','ok\n')

    def test_static_implied_do_expression(self):
        self.check('''program main
integer,parameter::dp=selected_real_kind(15,307)
real(kind=dp)::a(3)
integer::i
a=normalize([(i*i,i=1,size(a))]*1.0_dp)
print '(3f6.3)',a
contains
function normalize(x) result(y)
real(kind=dp),intent(in)::x(:)
real(kind=dp)::y(size(x))
y=x/sum(x)
end function normalize
end program main
''','0.071 0.286 0.643\n')

    def test_module_side_effects_persist(self):
        self.check('''module m
integer::calls=0
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
calls=calls+1
y=x
end function copy
end module m
program main
use m
integer::a(2,2),b(2,2)
a=1
b=copy(a)
b=copy(a)
print *,calls,sum(b)
end program main
''','2 4\n')

    def test_writable_arguments_and_scalar_result(self):
        self.check('''program main
integer::a(2,2),b(2,2),n,total
a=1
n=0
b=update(a,n)
print *,sum(a),sum(b),n
total=reset(a)
print *,sum(a),total
contains
function update(x,n) result(y)
integer,intent(inout)::x(:,:),n
integer::y(size(x,1),size(x,2))
x=x+1
n=n+1
y=x+1
end function update
function reset(x) result(y)
integer,intent(out)::x(2,2)
integer::y
x=5
y=sum(x)
end function reset
end program main
''','8 12 1\n20 20\n')
        self.reject('''program main
integer::a(2,2),n
a=1
n=update(a+1)
contains
function update(x) result(y)
integer,intent(inout)::x(:,:)
integer::y
x=x+1
y=sum(x)
end function update
end program main
''','writable function dummy')

    def test_scalar_arguments_and_negative_extent(self):
        self.check('''program main
print *,size(seq(-2)),size(seq(0)),seq(3)
print *,sum(grid())
contains
function seq(n) result(y)
integer,intent(in)::n
integer::y(n),i
do i=1,n
 y(i)=i
end do
end function seq
function grid() result(y)
integer::y(2,2)
y=7
end function grid
end program main
''','0 0 1 2 3\n28\n')

    def test_out_dummy_does_not_retain_old_values(self):
        self.check('''program main
integer::a(2,2),b(2,2)
a=1
b=reset(a)
contains
function reset(x) result(y)
integer,intent(out)::x(:,:)
integer::y(size(x,1),size(x,2))
x(1,1)=5
y=x
end function reset
end program main
''',error='uninitialized')

    def test_elemental_character_result_keeps_rank(self):
        self.check('''program main
character(len=2)::a(2,2)
character(len=3)::b(2,2)
a(1,1)='a1';a(2,1)='b2';a(1,2)='c3';a(2,2)='d4'
b=mark(a)
print '(4(a,1x))',b
print *,size(b,1),size(b,2)
contains
pure elemental function mark(x) result(y)
character(len=*),intent(in)::x
character(len=len(x)+1)::y
y=x//'!'
end function mark
end program main
''','a1! b2! c3! d4!\n2 2\n')

    def test_conformance_and_impure_forall_rejected(self):
        self.reject('''program main
integer::a(2,3),b(3,2),c(2,3)
a=1;b=1
c=copy(a+b)
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
y=x
end function copy
end program main
''','nonconforming')
        self.reject('''program main
integer::a(2,2),b(2),i
a=1
forall(i=1:2) b(i)=sum(copy(a))
contains
function copy(x) result(y)
integer,intent(in)::x(:,:)
integer::y(size(x,1),size(x,2))
y=x
end function copy
end program main
''','impure')

if __name__=='__main__':unittest.main()
