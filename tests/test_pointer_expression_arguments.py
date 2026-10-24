"""Pointer expression actuals preserve active extents and element order."""
import unittest

import test_deallocate
from test_record_read import ROOT


class PointerExpressionArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_mean_reproducer(self):
        source = (ROOT/'tests/programs/xacv_pointer_expression_actual.f90').read_text()
        for pure in (True, False):
            with self.subTest(pure=pure):
                self.check(source if pure else source.replace('pure function', 'function'), '6.500\n')
        self.check(source.replace('mean(ret_use**2)', 'mean(ret_use)'), '2.500\n')

    def test_kinds_strides_and_reassociation(self):
        for kind, one in [('integer(1)', '1_1'), ('integer(2)', '1_2'),
                          ('integer', '1'), ('integer(8)', '1_8'),
                          ('real', '1.0'), ('double precision', '1d0')]:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},target::x(5)
{kind},pointer::p(:)
x=[1,2,3,4,5]
p=>x(5:1:-2)
call consume(p+{one})
call consume(p)
p=>x(2:4:2)
call consume(p*({one}+{one}))
p=>x(2:1)
call consume(p+{one})
contains
subroutine consume(a)
{kind},intent(in)::a(:)
print *,size(a),int(sum(a)),lbound(a),ubound(a)
if(size(a)>0) print *,int(a(1))
end subroutine
end
''', '3 12 1 3\n6\n3 9 1 3\n5\n2 12 1 2\n4\n0 0 1 0\n')

    def test_logical_comparison(self):
        self.check('''implicit none
real,target::x(4)
real,pointer::p(:)
x=[-1.,2.,-3.,4.]
p=>x(4:1:-1)
call consume(p>0.)
contains
subroutine consume(a)
logical,intent(in)::a(:)
print *,size(a),count(a),a
end subroutine
end
''', '4 2 T F T F\n')

    def test_logical_pointer(self):
        self.check('''implicit none
logical,target::x(3)
logical,pointer::p(:)
x=[.true.,.false.,.true.]
p=>x(3:1:-1)
call consume(.not.p)
call consume(p)
contains
subroutine consume(a)
logical,intent(in)::a(:)
print *,count(a),a
end subroutine
end
''', '1 F T F\n2 T F T\n')

    def test_pointer_section_actual(self):
        self.check('''implicit none
integer,target::x(5)
integer,pointer::p(:)
x=[1,2,3,4,5]
p=>x(5:1:-1)
call consume(p(2:4:2)+1)
call consume(p(2:4:2))
contains
subroutine consume(a)
integer,intent(in)::a(:)
print *,size(a),a
end subroutine
end
''', '2 5 3\n2 4 2\n')

    def test_initialization_checks_on_selected_elements(self):
        source = '''implicit none
integer,target::x(3)
integer,pointer::p(:)
x(1)=2
x(3)=4
p=>x(1:3:2)
call consume(p+1)
contains
subroutine consume(a)
integer,intent(in)::a(:)
print *,sum(a)
end subroutine
end
'''
        self.check(source, '8\n')
        self.check(source.replace('x(1:3:2)', 'x(1:3)'),
                   error='uninitialized variable', reference=False)

    def test_rank_and_heap_destination_shape(self):
        for rank in (2, 3, 15):
            dims = ','.join(['2', '3']+['1']*(rank-2))
            axes = ','.join([':']*rank)
            section = ','.join(['2:1:-1', '1:3:2']+[':']*(rank-2))
            shape = ','.join(['2', '2']+['1']*(rank-2))
            with self.subTest(rank=rank):
                self.check(f'''implicit none
integer,target::x({dims})
integer,pointer::p({axes})
integer,allocatable::work({axes})
x=3
p=>x({section})
call consume(p+2)
call consume(p)
allocate(work({shape}))
work=p+3
print *,size(work),sum(work)
contains
subroutine consume(a)
integer,intent(in)::a({axes})
print *,size(a),sum(a)
end subroutine
end
''', '4 20\n4 12\n4 24\n')

    def test_unassociated_pointer_check(self):
        self.check('''implicit none
integer,pointer::p(:)
nullify(p)
call consume(p+1)
contains
subroutine consume(a)
integer,intent(in)::a(:)
print *,sum(a)
end subroutine
end
''', error='unassociated pointer', reference=False)

    def test_incompatible_pointer_shapes(self):
        self.check('''implicit none
integer,target::x(3),y(2)
integer,pointer::p(:),q(:)
x=1
y=2
p=>x
q=>y
call consume(p+q)
contains
subroutine consume(a)
integer,intent(in)::a(:)
print *,sum(a)
end subroutine
end
''', error='pointer array shapes do not conform', reference=False)


if __name__ == '__main__':
    unittest.main()
