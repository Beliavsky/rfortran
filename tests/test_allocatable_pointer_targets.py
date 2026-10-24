"""Allocated TARGET arrays alias POINTER descriptors in both execution modes."""
import unittest

import test_deallocate
from test_record_read import ROOT


class AllocatablePointerTargets(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_section_and_whole_target(self):
        source=(ROOT/'tests/programs/xacv_allocatable_pointer_target.f90').read_text()
        self.check(source,'2\n20 3\n1 20 3\n')
        self.check(source.replace('ret_use=>zret(i1:)','ret_use=>zret'),
                   '3\n20 2 3\n20 2 3\n')

    def test_lower_bounds_strides_aliases_and_overlap(self):
        self.check('''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:),q(:)
integer::n
read *,n
allocate(x(-2:n-3))
x=[1,2,3,4,5]
p=>x
print *,lbound(p),ubound(p),associated(p,x)
q=>p
p=p(n-3:-2:-1)
print *,x
p=>x(2:-2:-2)
print *,lbound(p),ubound(p),p
p=[10,20,30]
print *,q,x
nullify(p,q)
deallocate(x)
end
''','-2 2 T\n5 4 3 2 1\n1 3 1 3 5\n30 4 20 2 10 30 4 20 2 10\n',data='5\n')

    def test_runtime_ranks_and_nonunit_bounds(self):
        for rank in (2,3,7,15):
            axes=','.join([':']*rank)
            bounds=','.join(['-1:0','3:5']+['1']*(rank-2))
            first=','.join(['-1','3']+['1']*(rank-2))
            section=','.join(['0:-1:-1','3:5:2']+[':']*(rank-2))
            element=','.join(['2','1']+['1']*(rank-2))
            with self.subTest(rank=rank):
                self.check(f'''implicit none
integer,allocatable,target::x({axes})
integer,pointer::p({axes})
allocate(x({bounds}))
x=1
p=>x
p({first})=7
print *,size(p),sum(x),associated(p,x)
print *,lbound(p,1),ubound(p,1),lbound(p,2),ubound(p,2)
p=>x({section})
p({element})=9
print *,size(p),sum(p),x({first})
nullify(p)
deallocate(x)
end
''','6 12 T\n-1 0 3 5\n4 12 9\n')

    def test_heap_matrix_overlap_uses_runtime_snapshot(self):
        self.check('''implicit none
integer,allocatable,target::x(:,:)
integer,pointer::p(:,:)
allocate(x(-1:0,3:5))
x=reshape([1,2,3,4,5,6],[2,3])
p=>x
p=p(:,5:3:-1)
print *,x
p=p+1
print *,sum(x),associated(p,x)
end
''','5 6 3 4 1 2\n27 T\n')

    def test_types_and_kinds(self):
        for kind,one,two in [('integer(1)','1_1','2_1'),('integer(2)','1_2','2_2'),
                             ('integer','1','2'),('integer(8)','1_8','2_8'),
                             ('real','1.0','2.0'),('double precision','1d0','2d0')]:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},allocatable,target::x(:)
{kind},pointer::p(:)
allocate(x(3))
x={one}
p=>x(3:1:-1)
p(2)={two}
print *,int(sum(p)),int(x(2))
nullify(p)
deallocate(x)
end
''','4 2\n')
        self.check('''implicit none
logical,allocatable,target::x(:)
logical,pointer::p(:)
allocate(x(2))
x=.false.
p=>x
p(2)=.true.
print *,x,count(p)
end
''','F T 1\n')

    def test_zero_size_and_reassociation(self):
        self.check('''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
integer::n
do n=0,3
allocate(x(-2:n-3))
x=7
p=>x
print *,size(p),sum(p),associated(p),associated(p,x)
print *,lbound(p),ubound(p)
nullify(p)
deallocate(x)
end do
end
''','0 0 T F\n1 0\n1 7 T T\n-2 -2\n2 14 T T\n-2 -1\n3 21 T T\n-2 0\n')

    def test_unallocated_target(self):
        source='''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
integer::n
read *,n
if(n>0) allocate(x(n))
p=>x
print *,size(p)
end
'''
        self.check(source,'2\n',data='2\n')
        self.check(source,error='is unallocated',data='0\n',reference=False)

    def test_conformable_assignment_preserves_association(self):
        self.check('''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
allocate(x(3))
x=[1,2,3]
p=>x
x=[4,5,6]
print *,p,associated(p,x)
x=9
print *,p
nullify(p)
deallocate(x)
allocate(x(2))
x=[7,8]
p=>x
print *,p,size(p)
end
''','4 5 6 T\n9 9 9\n7 8 2\n')

    def test_allocatable_target_dummy_returns_association(self):
        self.check('''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
allocate(x(-1:1))
x=[1,2,3]
call bind(x,p)
p(1)=11
print *,x,associated(p,x(0:))
contains
subroutine bind(a,q)
integer,allocatable,target,intent(inout)::a(:)
integer,pointer,intent(out)::q(:)
q=>a(0:)
end subroutine
end
''','1 11 3 T\n')

    def test_pointer_initialization_mask_is_shared(self):
        source='''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
allocate(x(3))
p=>x
p(2)=17
print *,x(2)
end
'''
        self.check(source,'17\n')
        self.check(source.replace('p(2)=17','x(2)=17').replace('print *,x(2)','print *,p(2)'),
                   '17\n')
        self.check(source.replace('print *,x(2)','print *,p(1)'),
                   error='uninitialized variable',reference=False)

    def test_lifetime_and_readonly_targets_remain_checked(self):
        self.reject('''implicit none
integer,pointer::p(:)
call bad(p)
contains
subroutine bad(p)
integer,pointer,intent(out)::p(:)
integer,allocatable,target::local(:)
allocate(local(2))
p=>local
end subroutine
end
''','cannot outlive local TARGET')
        self.reject('''implicit none
integer,allocatable,target::x(:)
integer,pointer::p(:)
allocate(x(2))
x=1
call bad(x,p)
contains
subroutine bad(a,p)
integer,allocatable,target,intent(in)::a(:)
integer,pointer,intent(out)::p(:)
p=>a
p=2
end subroutine
end
''','cannot modify INTENT(IN) TARGET')

    def test_derived_targets_with_scalar_numeric_components(self):
        for rank in (1,2,15):
            axes=','.join([':']*rank)
            dims=','.join(['2']+['1']*(rank-1))
            first=','.join(['1']*rank)
            second=','.join(['2']+['1']*(rank-1))
            section=','.join(['2:1:-1']+[':']*(rank-1))
            with self.subTest(rank=rank):
                self.check(f'''implicit none
type point
integer::i
double precision::value
end type
type(point),allocatable,target::x({axes})
type(point),pointer::p({axes})
allocate(x({dims}))
x({first})%i=1
x({second})%i=2
x({first})%value=3d0
x({second})%value=4d0
p=>x({section})
p({first})%value=7d0
print *,p({first})%i,p({second})%i,x({second})%value
print *,associated(p,x({section}))
nullify(p)
deallocate(x)
end
''','2 1 7\nT\n')


if __name__=='__main__':
    unittest.main()
