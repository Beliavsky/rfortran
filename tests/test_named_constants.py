"""Named constants of supported intrinsic types and INTEGER kinds."""
import unittest
import test_deallocate
from test_record_read import ROOT

class NamedConstants(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_all_supported_types(self):
        self.check((ROOT/'tests/151_named_constants.f90').read_text(encoding='utf-8'),
                   'F T T T F\nF T T\n1 2 4 8\n12 1234 123456 10000000000\n'
                   '10000000000 10000000001\n1.25 2.5 5\n1.25 2.5 2.5 5\nabC\nab cd\n')

    def test_original_logical_literal(self):
        self.check('implicit none\nlogical,parameter::a=.false.\nprint *,a\nend\n','F\n')

    def test_logical_operators_relations_and_empty_reductions(self):
        self.check("""program main
logical,parameter::a=.true.,b=.false.
logical,parameter::c=.not.b .and. (a .eqv. a) .and. (a .neqv. b)
logical,parameter::d=1<2 .and. 2<=2 .and. 3>2 .and. 3>=3 .and. 2==2 .and. 1/=2
logical,parameter::e=1.5<2.0 .and. 'a'=='a ' .and. 'b'>'a'
logical,parameter::f=9007199254740993_8>9007199254740992_8
logical,parameter::g=all([logical::]),h=any([logical::])
logical,parameter::v(2)=[c,.not.d],w(2)=v
logical,parameter::j=any(mask=w,dim=1)
print *,c,d,e,f,g,h
print *,w
print *,j
end program main
""",'T T T T T F\nT F\nT\n')

    def test_module_imports_host_association_and_separate_parameter(self):
        self.check("""module m
logical,parameter::flag=.true.
integer(kind=8),parameter::large=10000000000_8
end module m
program main
use m,only:flag,large
logical::enabled
parameter(enabled=.not.flag)
logical,parameter::a(2)=[flag,enabled]
print *,enabled,a,large,kind(large)
call check_host()
contains
subroutine check_host()
logical,parameter::local=flag .and. .not.enabled
print *,local
end subroutine check_host
end program main
""",'F T F 10000000000 8\nT\n')

    def test_array_constant_expressions(self):
        # Array PARAMETERs and initializers defined by operators and
        # elemental intrinsics of other array constants.
        self.check("""program main
implicit none
integer, parameter :: n = 3
integer, parameter :: idx(n) = [3,1,2], twice(n) = 2*idx, shifted(n) = twice - idx + 10
integer, parameter :: joined(5) = [idx, 7, -idx(1)]
real, parameter :: roots(n) = sqrt(real(idx))
integer, parameter :: clipped(n) = max(idx, 2), rem(n) = mod(idx, 2)
logical, parameter :: big(n) = idx > 1
integer :: v(n) = 2*idx + 1
real :: w(n) = -real(idx)/2
print *, twice
print *, shifted
print *, joined
print '(3f8.4)', roots
print *, clipped, rem
print *, big
print *, v
print *, w
end program main
""", '6 2 4\n13 11 12\n3 1 2 7 -3\n  1.7321  1.0000  1.4142\n3 2 2 1 1 0\nT F T\n7 3 5\n-1.5 -0.5 -1.0\n')
        self.reject('program main\ninteger, parameter :: a(2) = [1,2], b(3) = [1,2,3], c(2) = a + b\nprint *, c\nend program main\n',
                    'shapes of array operands do not conform')

    def test_multidimensional_array_constants(self):
        # RESHAPE with PAD= and ORDER=, scalar fills, lower bounds, expressions
        # of other constants, constant elements, and run-time subscripts.
        self.check("""program main
implicit none
integer :: i, j
integer, parameter :: nb = 3
real(8), parameter :: p(2,nb) = reshape([1d0,2d0,3d0,4d0,5d0,6d0],[2,nb])
integer, parameter :: m(2,2) = 7, t(0:1,3) = reshape([(i, i=1,6)], [2,3])
integer, parameter :: c(3,2) = reshape([1,2,3,4], [3,2], pad=[0])
integer, parameter :: o(2,3) = reshape([1,2,3,4,5,6], [2,3], order=[2,1])
integer, parameter :: dbl(2,3) = 2*t
integer, parameter :: e12 = t(0,2), e13 = o(1,3)
integer, parameter :: v(3) = [10, 20, 30]
integer, parameter :: w(3,1) = reshape(v + 1, [3,1])
integer :: a(e13)
print *, p(:,2), p(2,nb)
print *, m
print *, t, lbound(t), ubound(t)
print *, c
print *, o
print *, dbl, e12, e13, size(a)
print *, w, shape(w)
do j = 1, 3
  print *, t(0,j), t(1,j), sum(p(:,j))
end do
end program main
""", '3.0 4.0 6.0\n7 7 7 7\n1 2 3 4 5 6 0 1 1 3\n1 2 3 4 0 0\n1 4 2 5 3 6\n'
            '2 4 6 8 10 12 3 3 3\n11 21 31 3 1\n1 2 3.0\n3 4 7.0\n5 6 11.0\n')
        for source, message in [
            ('integer, parameter :: a(2,2) = [1,2,3]', 'shape mismatch'),
            ('integer, parameter :: a(2,3) = reshape([1,2,3,4], [2,3])', 'RESHAPE'),
            ('integer, parameter :: a(2,2) = 1, b = a(3,1)', 'out of bounds'),
        ]:
            with self.subTest(source=source):
                self.reject('program main\n'+source+'\nprint *, a\nend program main\n', message)

    def test_invalid_initializers_ranges_and_writes(self):
        for source,message in [
            ('logical,parameter::a=1','logical constant expression'),
            ('integer::v\nlogical,parameter::a=.false. .and. v>0','constant expression'),
            ('integer(kind=1),parameter::a=128','out of range'),
            ('integer(kind=2),parameter::a(1)=[32768]','out of range'),
            ('real,parameter::a=1.0d100',"REAL kind's range"),
        ]:
            with self.subTest(source=source):
                self.reject('program main\n'+source+'\nend program main\n',message)
        self.reject('logical,parameter::a=.true.\na=.false.\nend\n','cannot modify PARAMETER')

if __name__ == '__main__':
    unittest.main()
