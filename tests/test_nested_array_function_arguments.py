"""Bind nested function results before associating an outer procedure call."""
import unittest

import test_deallocate
from test_record_read import ROOT


class NestedArrayFunctionArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    @staticmethod
    def source():
        return (ROOT/'tests/programs/xacv_nested_runtime_array_function.f90').read_text()

    def test_repro_and_controls(self):
        original = self.source()
        variants = {
            'original': original,
            'required': original.replace('make_matrix(xx,good)', 'make_matrix(xx)')
                                .replace('logical,intent(in),optional::good(:,:)\n', '')
                                .replace('if(present(good)) yy=0d0\n', ''),
            'fixed': original.replace('double precision,allocatable::xx(:,:)',
                                      'double precision::xx(3,2)').replace('allocate(xx(n,2))', ''),
            'empty': original.replace('allocate(xx(n,2))', 'allocate(xx(n,0))'),
        }
        for name, program in variants.items():
            with self.subTest(variant=name):
                self.check(program, '0 0 0\n' if name == 'empty' else '2 2 4\n')

    def test_two_function_levels(self):
        source = self.source().replace('yy=1d0', 'yy=sum(xx)')
        source = source.replace('call consume(make_matrix(xx))',
                                'call consume(make_matrix(copy_matrix(xx)))')
        source = source.replace('end module', '''function copy_matrix(xx) result(yy)
double precision,intent(in)::xx(:,:)
double precision::yy(size(xx,1),size(xx,2))
yy=xx+1d0
end function
end module''')
        self.check(source, '2 2 48\n')

    def test_function_evaluated_once_before_consumer(self):
        self.check(self.source().replace('yy=1d0', 'print *,999\nyy=1d0'),
                   '999\n2 2 4\n')

    def test_function_result_in_array_expression(self):
        self.check(self.source().replace('call consume(make_matrix(xx))',
                                        'call consume(2d0*make_matrix(xx)+1d0)'),
                   '2 2 12\n')

    def test_repeated_calls_and_changing_result_shape(self):
        source = self.source().replace('n=3\nallocate(xx(n,2))\nxx=1d0\ncall outer(xx)',
                                      'do n=0,3\nallocate(xx(2,n))\nxx=1d0\ncall outer(xx)\ndeallocate(xx)\nend do')
        self.check(source, '0 0 0\n1 1 1\n2 2 4\n3 3 9\n')

    def test_explicit_pack_actual(self):
        self.check('''implicit none
integer,allocatable::x(:)
integer::n
n=4
allocate(x(n))
x=[-1,2,-3,4]
call outer(x)
contains
subroutine outer(x)
integer,intent(in)::x(:)
call consume(increment(pack(x,x>0)))
call consume(increment(pack(x,x< -10)))
end subroutine
function increment(x) result(y)
integer,intent(in)::x(:)
integer::y(size(x))
y=x+1
end function
subroutine consume(y)
integer,intent(in)::y(:)
print *,size(y),sum(y)
end subroutine
end
''', '2 8\n0 0\n')

    def test_result_types_and_ranks(self):
        for rank in (2, 3, 15):
            for kind, value, expression, reduction, expected in (
                ('integer', '1', 'x+1', 'sum(y)', '4'),
                ('real', '1.0', 'x+1.0', 'int(sum(y))', '4'),
                ('double precision', '1d0', 'x+1d0', 'int(sum(y))', '4'),
                ('logical', '.true.', '.not.x', 'count(y)', '0'),
            ):
                with self.subTest(rank=rank, kind=kind):
                    axes = ','.join([':']*rank)
                    allocation = ','.join(['n']+['1']*(rank-1))
                    result = ', &\n &'.join(f'size(x,{i})' for i in range(1,rank+1))
                    self.check(f'''implicit none
{kind},allocatable::x({axes})
integer::n
read *,n
allocate(x({allocation}))
x={value}
call outer(x)
contains
subroutine outer(x)
{kind},intent(in)::x({axes})
call consume(make(x))
end subroutine
function make(x) result(y)
{kind},intent(in)::x({axes})
{kind}::y({result})
y={expression}
end function
subroutine consume(y)
{kind},intent(in)::y({axes})
print *,size(y),{reduction}
end subroutine
end
''', f'2 {expected}\n', data='2\n')


if __name__ == '__main__':
    unittest.main()
