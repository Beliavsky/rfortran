"""General-rank pointer descriptors in the interpreter, native O0/O2 and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT


class PointerDescriptors(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/136_pointer_descriptors.f90').read_text(encoding='utf-8'),
                   '3 1 2 3 9 24\n2 2 2 24\n2 12 8\n5 9 3 4 1 2\n')

    def test_ranks_two_through_fifteen(self):
        for rank in range(2, 16):
            with self.subTest(rank=rank):
                dims = ','.join(['2'] + ['1'] * (rank-1))
                deferred = ','.join([':'] * rank)
                first = ','.join(['1'] * rank)
                second = ','.join(['2'] + ['1'] * (rank-1))
                self.check(f'''program main
integer,target::a({dims})
integer,pointer::p({deferred})
a=7
p=>a
p({second})=11
print *,size(p),p({first}),a({second}),sum(p)
print *,shape(p)
end
''', '2 7 11 18\n' + ' '.join(['2']+['1']*(rank-1))+'\n')

    def test_lower_bounds_and_changed_shape(self):
        self.check('''program main
integer,target::a(-1:0,2:4),b(3,1)
integer,pointer::p(:,:),q(:,:)
a=reshape([1,2,3,4,5,6],[2,3])
b=8
p=>a
print *,shape(p),lbound(p),ubound(p)
p(0,3)=40
q=>p
p=>b
q(-1,2)=10
print *,q
print *,shape(p),p
nullify(p)
print *,associated(p),associated(q),q(0,3)
end
''', '2 3 -1 2 0 4\n10 2 3 40 5 6\n3 1 8 8 8\nF T 40\n')

    def test_strided_matrix_section_and_overlapping_assignment(self):
        self.check('''program main
integer,target::a(2,3)
integer,pointer::p(:,:),q(:,:)
a=reshape([1,2,3,4,5,6],[2,3])
p=>a(:,3:1:-1)
q=>a
print *,p
p=q
print *,a
p(:,2:3)=p(:,1:2)
print *,a
end
''', '5 6 3 4 1 2\n5 6 3 4 1 2\n3 4 1 2 1 2\n')

    def test_runtime_section_bounds_and_capture(self):
        self.check('''program main
integer,target::a(3,4,2)
integer,pointer::p(:,:)
integer::lo,hi,k
a=reshape([(k,k=1,24)],[3,4,2])
lo=2;hi=4;k=2
p=>a(2,lo:hi:2,k:1:-1)
lo=1;hi=1;k=1
print *,shape(p),p
p(2,2)=99
print *,a(2,4,1)
end
''','2 2 17 23 5 11\n99\n')

    def test_matrix_output_pointer(self):
        self.check('''program main
real(8),target::a(2,3,2)
real(8),pointer::p(:,:)
a=1d0
call plane(a,2,p)
p(2,3)=9d0
print *,shape(p),a(2,3,2),sum(p)
contains
subroutine plane(a,k,p)
real(8),target,intent(in)::a(:,:,:)
integer,intent(in)::k
real(8),pointer,intent(out)::p(:,:)
p=>a(:,:,k)
end subroutine
end
''','2 3 9 14\n')

    def test_no_element_count_limit(self):
        self.check('''program main
integer,target::a(100,100)
integer,pointer::p(:,:)
a=2
p=>a
p=p+1
print *,size(p),sum(p),a(100,100)
end
''','10000 30000 3\n')

    def test_uninitialized_target_element(self):
        self.check('''program main
integer,target::a(2,2)
integer,pointer::p(:,:)
p=>a
p(1,1)=7
print *,p(1,1)
print *,p(2,2)
end
''', error='uninitialized variable `a`: element 4')

    def test_bad_dimension_index_and_shape(self):
        self.check('''program main
integer,target::a(2,3)
integer,pointer::p(:,:)
a=1
p=>a
print *,p(3,1)
end
''',error='subscript outside bounds in dimension 1')
        self.check('''program main
integer,target::a(2,3),b(3,2)
integer,pointer::p(:,:),q(:,:)
a=1;b=2
p=>a;q=>b
p=q
end
''',error='shapes do not conform')

    def test_constructor_assignment_and_runtime_dimension(self):
        self.check('''program main
integer,target::a(2,3)
integer,pointer::p(:,:),q(:)
integer::d
p=>a
p=reshape([1,2,3,4,5,6],[2,3])
d=2
print *,size(dim=d,array=p),lbound(array=p,dim=d),ubound(p,d)
q=>a(:,2)
q=[8,9]
print *,p
end
''','3 1 3\n1 2 8 9 5 6\n')

    def test_reductions_and_mixed_expression_kind(self):
        self.check('''program main
integer,target::a(2,2)
integer,pointer::p(:,:)
logical,target::mask(2,2)
logical,pointer::q(:,:)
a=2;mask=.true.
p=>a;q=>mask
q(1,2)=.false.
print *,minval(p),maxval(p),product(p),sum(p+0.25d0)
print *,any(q),all(q),count(q)
end
''','2 2 16 9\nT F 3\n')

    def test_rank_limit(self):
        rank=','.join([':']*16)
        self.reject(f'integer,pointer::p({rank})\nend\n','rank at most 15')


if __name__ == '__main__':
    unittest.main(verbosity=2)
