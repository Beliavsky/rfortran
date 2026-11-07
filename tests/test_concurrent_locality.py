"""DO CONCURRENT locality: serial interpreter/native semantics and gfortran."""
from pathlib import Path
import unittest
import test_deallocate

SOURCE=(Path(__file__).parent/'programs/xconcurrent_locality.f90').read_text()

class ConcurrentLocality(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check(SOURCE,'12 16 22 30\n99 -1 10\n')

    def test_plain_concurrent_index_scope(self):
        self.check('''implicit none
integer :: i,a(3)
i=81
do concurrent(i=1:3)
a(i)=i
end do
print *,a,i
end
''','1 2 3 81\n')

    def test_fixed_array_local_init(self):
        self.check('''implicit none
integer :: i,t(-1:0),a(3)
t=[5,6]
do concurrent(i=1:3) local_init(t) shared(a) default(none)
t(-1)=t(-1)+i
a(i)=sum(t)
end do
print *,a,t
end
''','12 13 14 5 6\n')

    def test_character_local(self):
        self.check('''implicit none
integer :: i
character(2) :: text='ab',a(2)
do concurrent(i=1:2) local_init(text) shared(a) default(none)
text(1:1)='X'
a(i)=text
end do
print *,a,text
end
''','XbXbab\n')

    def test_shared_derived(self):
        self.check('''implicit none
type :: point
integer :: x
end type
type(point) :: a(2)
integer :: i
do concurrent(i=1:2) shared(a) default(none)
a(i)%x=i
end do
print *,a%x
end
''','1 2\n')

    def test_named_cycle_and_mask(self):
        self.check('''implicit none
integer :: i,j,t,a(2,3)
a=0
outer: do concurrent(i=1:2,j=1:3,j/=2) local(t) shared(a) default(none)
if(i==2) cycle outer
t=i+j
a(i,j)=t
end do outer
print *,a
end
''','2 0 0 0 4 0\n')

    def test_reduce_with_locality(self):
        self.check('''implicit none
integer :: i,t,s=10
do concurrent(i=1:4) local(t) reduce(+:s) default(none)
t=i*i
s=s+t
end do
print *,s
end
''','40\n',reference=False)

    def test_parameter_exempt(self):
        self.check('''implicit none
integer,parameter :: n=2
integer :: i,a(n)
do concurrent(i=1:n) shared(a) default(none)
a(i)=i+n
end do
print *,a
end
''','3 4\n')

    def test_nested_index_scope(self):
        self.check("""implicit none
integer :: i,j,a(2,2)
i=71
j=72
do concurrent(i=1:2) shared(a,j) default(none)
do concurrent(j=1:2) shared(a,i) default(none)
a(i,j)=10*i+j
end do
end do
print *,a,i,j
end
""",'11 21 12 22 71 72\n')

    def test_multiple_local_variables(self):
        self.check("""implicit none
integer :: i,j,k,a(2)
do concurrent(i=1:2) local(j,k) shared(a) default(none)
j=i
k=2*i
a(i)=j+k
end do
print *,a
end
""",'3 6\n')

    def test_implicit_missing_locality(self):
        self.reject("""integer :: i
do concurrent(i=1:2) default(none)
x=real(i)
end do
end
""",'DEFAULT(NONE) requires a locality specifier for `x`')

    def test_implicit_read_missing_locality(self):
        self.reject("""integer :: i,a(2)
x=3.0
do concurrent(i=1:2) shared(a) default(none)
a(i)=int(x)
end do
end
""",'DEFAULT(NONE) requires a locality specifier for `x`')

    def test_custom_implicit_index(self):
        self.check("""implicit integer(q)
integer :: a(2)
do concurrent(q=1:2)
a(q)=q
end do
print *,a
end
""",'1 2\n')

    def test_block_local_exempt_from_default_none(self):
        self.check("""implicit none
integer :: i,a(2)
do concurrent(i=1:2) shared(a) default(none)
block
integer :: t
t=2*i
a(i)=t
end block
end do
print *,a
end
""",'2 4\n')

    def test_empty_construct_preserves_outside(self):
        self.check("""implicit none
integer :: i=42,t=7
do concurrent(i=1:0) local_init(t) default(none)
t=t+i
end do
print *,i,t
end
""",'42 7\n')

    def test_local_is_reset_each_iteration(self):
        self.check("""implicit none
integer :: i,t,a(2)
do concurrent(i=1:2) local(t) shared(a) default(none)
if(i==1) t=4
a(i)=t
end do
print *,a
end
""",error='uninitialized variable',reference=False)

    def test_missing_locality(self):
        self.reject(SOURCE.replace('shared(values)',''), 'DEFAULT(NONE) requires a locality specifier for `values`')

    def test_duplicate_locality(self):
        self.reject(SOURCE.replace('local(scratch)','local(scratch) shared(scratch)'), 'duplicate locality specifier')

    def test_duplicate_default(self):
        self.reject(SOURCE.replace('default(none)','default(none) default(none)'), 'duplicate DEFAULT(NONE)')

    def test_index_in_clause(self):
        self.reject(SOURCE.replace('shared(values)','shared(values,i)'), 'cannot appear in a locality specifier')

    def test_local_header_reference(self):
        self.reject(SOURCE.replace('i=1:4','i=1:scratch'), 'cannot appear in the concurrent header')

    def test_local_does_not_copy_outer_value(self):
        self.reject(SOURCE.replace('scratch=i*i','offset=offset+scratch'), 'uninitialized variable')

    def test_runtime_local_array(self):
        # The installed gfortran miscompiles/hangs on this runtime LOCAL case.
        # Check interpreter and native results against the known answer.
        self.check('''program p
implicit none
integer::a(3)
call f(2,a)
print *,a
contains
subroutine f(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
do concurrent(i=1:size(a)) local(work)
work=i
a(i)=sum(work)
end do
end subroutine
end program
''','2 4 6',reference=False)

if __name__=='__main__':
    unittest.main()
