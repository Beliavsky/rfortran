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
