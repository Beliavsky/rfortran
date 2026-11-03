"""Native ELEMENTAL calls in descriptor-backed POINTER array expressions."""
import unittest
import test_deallocate
from test_record_read import ROOT


class PointerElementalFunctions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fve_metrics(self):
        self.check((ROOT / 'tests/programs/xfve_pointer_metrics.f90').read_text(),
                   '0.565952 0.944911 0.866025 0.424196 0.003750\n')

    def test_logical_results_and_general_rank(self):
        for rank in (2, 3, 15):
            dimensions = ','.join(['2', '3'] + ['1'] * (rank-2))
            axes = ','.join([':'] * rank)
            section = ','.join(['2:1:-1', '1:3:2'] + [':'] * (rank-2))
            with self.subTest(rank=rank):
                self.check(f'''implicit none
integer,target::x({dimensions})
integer,pointer::p({axes})
x=reshape([1,2,3,4,5,6],[{dimensions}])
p=>x({section})
print *,size(p),count(positive(p)),all(positive(p).eqv.(p>3))
contains
elemental function positive(v) result(y)
integer,intent(in)::v
logical::y
y=v>3
end function
end
''', '4 2 T\n')

    def test_numeric_result_kind(self):
        self.check('''implicit none
integer,target::x(3)
integer,pointer::p(:)
x=[1,2,3]
p=>x(3:1:-1)
print '(3f6.1)',half(p)
print *,sum(bucket(half(p)))
contains
elemental function half(v) result(y)
integer,intent(in)::v
double precision::y
y=0.5d0*v
end function
elemental function bucket(v) result(y)
double precision,intent(in)::v
integer::y
y=int(v)
end function
end
''', '1.5 1.0 0.5\n2\n')

    def test_numeric_input_kinds(self):
        for kind in ['integer(1)', 'integer(2)', 'integer', 'integer(8)',
                     'real', 'double precision']:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},target::x(5)
{kind},pointer::p(:)
x=[1,2,3,4,5]
p=>x(5:1:-2)
print *,int(sum(bump(p)))
contains
elemental function bump(v) result(y)
{kind},intent(in)::v
{kind}::y
y=v+1
end function
end
''', '12\n')

    def test_logical_inputs(self):
        self.check('''implicit none
logical,target::x(3)
logical,pointer::p(:)
x=[.true.,.false.,.false.]
p=>x(3:1:-1)
print *,count(invert(p)),invert(p)
contains
elemental function invert(v) result(y)
logical,intent(in)::v
logical::y
y=.not.v
end function
end
''', '2 T T F\n')

    def test_impure_elemental_invocations(self):
        self.check('''module m
integer::calls=0
contains
impure elemental function tick(v) result(y)
integer,intent(in)::v
integer::y
calls=calls+1
y=2*v
end function
end module
program ptest
use m
implicit none
integer,target::x(3)
integer,pointer::p(:)
integer::result
x=[1,2,3]
p=>x
result=sum(tick(p))
print *,result,calls
p=>x(2:1)
result=sum(tick(p))
print *,result,calls
end
''', '12 3\n0 3\n')

    def test_scalar_actual_evaluated_once(self):
        self.check('''module m
integer::calls=0
contains
function bias() result(y)
real::y
calls=calls+1
y=2.0
end function
elemental function affine(b,v) result(y)
real,intent(in)::b,v
real::y
y=b+v
end function
end module
program ptest
use m
implicit none
real,target::x(5)
real,pointer::p(:)
real::result,base
x=[1.,2.,3.,4.,5.]
p=>x(1:5:2)
result=sum(affine(bias(),p))
print *,result,calls
base=2.0
result=sum(affine(exp(log(base)),p))
print *,result,calls
p=>x(2:1)
result=sum(affine(bias(),p))
print *,result,calls
end
''', '15.0 1\n15.0 1\n0.0 2\n')

    def test_nested_calls_and_reassociation(self):
        self.check('''implicit none
integer,target::x(3)
integer,pointer::p(:)
x=[1,2,3]
p=>x
print *,sum(twice(twice(p)))
p=>x(3:1:-2)
print *,twice(p+1)
contains
elemental function twice(v) result(y)
integer,intent(in)::v
integer::y
y=2*v
end function
end
''', '24\n8 4\n')

    def test_checks(self):
        source = '''implicit none
real,target::x(3),z(2)
real,pointer::p(:),q(:)
x=1.
z=2.
p=>x
q=>z
print *,sum(combine(p,q))
contains
elemental function combine(a,b) result(y)
real,intent(in)::a,b
real::y
y=a+b
end function
end
'''
        self.check(source, error='pointer array shapes do not conform', reference=False)
        self.check(source.replace('p=>x', 'nullify(p)'), error='unassociated pointer', reference=False)
        self.check(source.replace('q=>z', 'q=>x').replace('x=1.', 'x(1)=1.'),
                   error='uninitialized variable', reference=False)


if __name__ == '__main__':
    unittest.main(verbosity=2)
