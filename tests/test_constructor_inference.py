"""Constructor element types, generic dispatch, and lowering diagnostics."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module kinds_mod
interface identify
module procedure ints,wide,reals,doubles,logicals,chars
end interface
contains
pure integer function ints(x) result(n)
integer,intent(in)::x(:)
n=10+size(x)
end function
pure integer function wide(x) result(n)
integer(8),intent(in)::x(:)
n=20+size(x)
end function
pure integer function reals(x) result(n)
real,intent(in)::x(:)
n=30+size(x)
end function
pure integer function doubles(x) result(n)
double precision,intent(in)::x(:)
n=40+size(x)
end function
pure integer function logicals(x) result(n)
logical,intent(in)::x(:)
n=50+size(x)
end function
pure integer function chars(x) result(n)
character(len=*),intent(in)::x(:)
n=60+size(x)
end function
end module
"""


class ConstructorInference(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_types_kinds_and_nested_implied_do(self):
        self.check(MODULE+"""use kinds_mod
integer::i,j
print *,identify([(i,i=1,3)]),identify([(2_8,i=1,3)])
print *,identify([(real(i),i=1,2)]),identify([(dble(i),i=1,2)])
print *,identify([(i>1,i=1,3)]),identify([('xy',i=1,2)])
print *,identify([((j,j=1,i),i=1,3)])
print *,identify([integer::]),identify([real::]),identify([logical::])
end
""", '13 23\n32 42\n53 62\n16\n10 30 50\n')

    def test_scalar_and_array_elements(self):
        self.check(MODULE+"""use kinds_mod
integer::v(2),i
v=[2,3]
print *,identify([1,v,4]),identify([v,[5,6]])
print *,identify([(v,i=1,2)])
end
""", '14 14\n14\n')

    def test_mixed_types_and_kinds_rejected(self):
        for expression in ['[1,2.0]', '[1,2_8]', '[.true.,1]', "['a',1]"]:
            with self.subTest(expression=expression):
                self.reject(MODULE+'use kinds_mod\nprint *,identify('+expression+')\nend\n', 'same type and kind')
        self.reject(MODULE+"use kinds_mod\nprint *,identify(['a','bb'])\nend\n", 'CHARACTER length')

    def test_runtime_constructor_reaches_array_argument_lowering(self):
        self.check((ROOT/'xset_alloc_implied_do.f90').read_text(encoding='utf-8'), '1 2 3\n')

    def test_set_alloc_with_constant_bound(self):
        source=(ROOT/'xset_alloc_implied_do.f90').read_text(encoding='utf-8')
        source=source.replace('integer :: i, n', 'integer :: i\ninteger, parameter :: n=3').replace('\nn = 3\n', '\n')
        self.check(source, '1 2 3\n')


if __name__ == '__main__':
    unittest.main(verbosity=2)
