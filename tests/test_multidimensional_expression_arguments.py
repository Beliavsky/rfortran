"""Array expression actuals preserve type, kind, rank and active shape."""
import unittest

import test_deallocate
from test_record_read import ROOT


class MultidimensionalExpressionArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_xacv_mask_reproducer_and_controls(self):
        source = (ROOT/'tests/programs/xacv_runtime_logical_mask_actual.f90').read_text()
        variants = {
            'original': source,
            'comparison': source.replace('abs(xx-bad_value)>1.0e-10_dp', 'xx/=bad_value'),
            'positional': source.replace('good=abs', 'abs'),
            'required': source.replace('logical,intent(in),optional::good', 'logical,intent(in)::good')
                              .replace('if(present(good)) print', 'print'),
            'fixed': source.replace('real(dp),allocatable::xx(:,:)', 'real(dp)::xx(2,2)')
                           .replace('allocate(xx(n,2))', ''),
        }
        for name, program in variants.items():
            with self.subTest(variant=name):
                self.check(program, '2 2 3\n')

    def test_numeric_types_and_kinds(self):
        for kind, value, add in [('integer(1)', '1_1', '2_1'),
                                  ('integer(2)', '1_2', '2_2'),
                                  ('integer', '1', '2'),
                                  ('integer(8)', '1_8', '2_8'),
                                  ('real', '1.0', '2.0'),
                                  ('double precision', '1d0', '2d0')]:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},allocatable::x(:,:)
integer::n
read *,n
allocate(x(-1:n-2,3:5))
x={value}
call consume(x+{add})
contains
subroutine consume(a)
{kind},intent(in)::a(:,:)
print *,shape(a),int(sum(a)),lbound(a)
end subroutine
end
''', '2 3 18 1 1\n', data='2\n')

    def test_runtime_ranks_and_empty_masks(self):
        for rank in (2, 3, 15):
            axes = ','.join([':']*rank)
            allocation = ','.join(['-1:n-2', '3:4']+['1']*(rank-2))
            first = ','.join(['-1', '3']+['1']*(rank-2))
            for n in (0, 2):
                with self.subTest(rank=rank, extent=n):
                    self.check(f'''implicit none
double precision,allocatable::x({axes})
integer::n
read *,n
allocate(x({allocation}))
x=1d0
if(n>0) x({first})=-999d0
call consume(abs(x+999d0)>1d-10)
contains
subroutine consume(good)
logical,intent(in)::good({axes})
print *,size(good),count(good),lbound(good,1),ubound(good,1)
end subroutine
end
''', '0 0 1 0\n' if n == 0 else '4 3 1 2\n', data=f'{n}\n')

    def test_scalar_first_intrinsic_and_conversions(self):
        self.check('''implicit none
integer,allocatable::x(:,:)
integer::n
read *,n
allocate(x(n,2))
x=3
call consume(max(2,x))
call consume(int(real(x,kind=kind(1d0)),kind=kind(x)))
call consume_double(real(x,kind=kind(1d0)))
call consume_wide(int(x,kind=selected_int_kind(18)))
contains
subroutine consume(a)
integer,intent(in)::a(:,:)
print *,shape(a),sum(a)
end subroutine
subroutine consume_double(a)
double precision,intent(in)::a(:,:)
print *,shape(a),int(sum(a))
end subroutine
subroutine consume_wide(a)
integer(selected_int_kind(18)),intent(in)::a(:,:)
print *,shape(a),sum(a)
end subroutine
end
''', '2 2 12\n2 2 12\n2 2 12\n2 2 12\n', data='2\n')

    def test_elemental_logical_result(self):
        self.check('''implicit none
integer,allocatable::x(:,:)
integer::n
read *,n
allocate(x(n,2))
x=2
x(1,1)=-1
call consume(positive(x))
contains
elemental logical function positive(a)
integer,intent(in)::a
positive=a>0
end function
subroutine consume(a)
logical,intent(in)::a(:,:)
print *,shape(a),count(a)
end subroutine
end
''', '2 2 3\n', data='2\n')

    def test_logical_expression_and_forwarding(self):
        self.check('''implicit none
logical,allocatable::x(:,:)
integer::n
read *,n
allocate(x(n,2))
x=.true.
x(1,1)=.false.
call outer(.not.x)
contains
subroutine outer(a)
logical,intent(in)::a(:,:)
call consume(a .or. .false.)
end subroutine
subroutine consume(a)
logical,intent(in)::a(:,:)
print *,shape(a),count(a)
end subroutine
end
''', '2 2 1\n', data='2\n')

    def test_equal_size_different_shape_is_rejected_at_runtime(self):
        self.check('''implicit none
integer,allocatable::a(:,:),b(:,:)
integer::n
read *,n
allocate(a(n,3),b(3,n))
a=1
b=2
call consume(a+b)
contains
subroutine consume(x)
integer,intent(in)::x(:,:)
print *,sum(x)
end subroutine
end
''', error='array shapes do not conform', data='2\n', reference=False)

    def test_repeated_calls_with_changing_extents(self):
        self.check('''implicit none
integer,allocatable::x(:,:)
integer::n
do n=1,3
allocate(x(n,2))
x=n
call consume(x+1)
deallocate(x)
end do
contains
subroutine consume(a)
integer,intent(in)::a(:,:)
print *,shape(a),sum(a)
end subroutine
end
''', '1 2 4\n2 2 12\n3 2 24\n')


if __name__ == '__main__':
    unittest.main()
